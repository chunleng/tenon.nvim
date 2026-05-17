use crate::{
    clients::{ChatAgent, StreamItem, SupportedModels, get_agent},
    config::user::WorkflowConfig,
    directive::{Directive, DirectiveSource},
    get_application_config, get_workflow_registry,
    tools::resolve_tools,
    utils::GLOBAL_EXECUTION_HANDLER,
};
use chrono::{DateTime, Local};
use nvim_oxi::{Result as OxiResult, api::types::LogLevel};
use rig::{completion::Usage, message::ToolResultContent};
use std::{
    collections::HashMap,
    sync::atomic::{AtomicBool, Ordering},
    sync::{Arc, LazyLock, Mutex, RwLock},
};

pub mod history;
pub mod log;
pub mod log_indexer;
pub mod workflow;

pub use log::{
    TenonAssistantMessage, TenonAssistantMessageContent, TenonLog, TenonLogData, TenonToolCall,
    TenonToolError, TenonToolLog, TenonToolResult, TenonUserMessage, TenonUserTextMessage,
    TenonWorkflowLog,
};
pub use log_indexer::ChatLogIndexer;

use history::save_to_history;

/// Builds a workflow-wrapped prompt if there's an active workflow.
fn build_workflow_prompt(
    active_workflow: &Arc<RwLock<Option<ActiveWorkflow>>>,
    base_prompt: String,
) -> String {
    if let Ok(active_lock) = active_workflow.read()
        && let Some(active) = active_lock.as_ref()
    {
        let registry = get_workflow_registry();
        let workflow = registry
            .get(&active.id)
            .expect("active workflow id must exist in workflow registry");
        let total_steps = workflow.steps.len();
        if let Some(step) = workflow.steps.get(active.step - 1) {
            let mut goto_lines: Vec<String> = step
                .goto_instructions
                .iter()
                .map(|instr| {
                    let condition = instr
                        .condition
                        .as_ref()
                        .map(|x| format!("{} → ", x))
                        .unwrap_or_default();
                    let target_step = instr.to.resolve_step_index(active.step);
                    match target_step {
                        None => format!("{}end_workflow output:{}", condition, instr.output),
                        Some(step) if step > total_steps => {
                            format!("{}end_workflow output:{}", condition, instr.output)
                        }
                        Some(step) => {
                            format!(
                                "{}navigate_output step:{} output:{}",
                                condition, step, instr.output
                            )
                        }
                    }
                })
                .collect();

            // Only add default ending if at last step and no goto already ends workflow
            if active.step == total_steps {
                let has_ending_goto = step.goto_instructions.iter().any(|instr| {
                    let target_step = instr.to.resolve_step_index(active.step);
                    match target_step {
                        None => true,
                        Some(s) => s > total_steps,
                    }
                });
                if !has_ending_goto {
                    goto_lines.push("end_workflow output:nothing".to_string());
                }
            }

            let goto_instruction = goto_lines.join("\n");

            // Build memory section if there's stored memory
            let memory_section = if active.memory.is_empty() {
                String::new()
            } else {
                let memory_entries: Vec<String> = active
                    .memory
                    .iter()
                    .map(|(k, v)| format!("<memory name=\"{}\">{}</memory>", k, v))
                    .collect();
                memory_entries.join("\n")
            };

            return format!(
                "<context>\n\
                    Currently in {} step of {} workflow. Priority: user prompt > workflow instruction > chat history\n\
                    Current step instruction:\n\
                    <instruction>\n\
                    {}\n\
                    </instruction>\n\
                    If asking user question → stop, do not navigate.\n\
                    After completing instruction → call navigate_workflow with step number + step_output.\n\
                    <navigation>\n\
                    {}\n\
                    </navigation>\n\
                    {}</context>\n\
                    {}",
                step.title,
                workflow.title,
                step.instruction.resolve().unwrap_or_default(),
                goto_instruction,
                memory_section,
                base_prompt
            );
        }
    }
    format!(
        "<context>Currently not in workflow</context>\n{}",
        base_prompt
    )
}

pub static CHAT_SESSIONS: LazyLock<Mutex<Vec<Arc<RwLock<ChatSession>>>>> =
    LazyLock::new(|| Mutex::new(Vec::new()));

/// Returns the chat session at `index`, creating new ones as needed.
pub fn get_or_create_chat_session(index: usize) -> Arc<RwLock<ChatSession>> {
    let mut sessions = CHAT_SESSIONS.lock().unwrap();
    while sessions.len() <= index {
        sessions.push(Arc::new(RwLock::new(ChatSession::new())));
    }
    sessions[index].clone()
}

/// Removes the chat session at `index`, shifting subsequent indices down.
pub fn remove_chat_session(index: usize) {
    let mut sessions = CHAT_SESSIONS.lock().unwrap();
    if index < sessions.len() {
        sessions.remove(index);
    }
}

/// Returns the current number of chat sessions.
pub fn chat_session_count() -> usize {
    CHAT_SESSIONS.lock().unwrap().len()
}

fn generate_chat_id() -> String {
    let now = Local::now();
    let datetime = now.format("%Y-%m-%dT%H:%M:%S");
    let hash = format!("{:08x}", now.timestamp_subsec_nanos());
    format!("{}_{}", datetime, hash)
}

#[derive(Debug, Clone)]
pub struct ActiveAgent {
    pub name: String,
    pub inner: TenonAgent,
}

impl std::ops::Deref for ActiveAgent {
    type Target = TenonAgent;
    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

#[derive(Debug, Clone)]
pub struct ActiveWorkflow {
    pub id: String,
    pub step: usize,
    pub memory: HashMap<String, String>,
}

impl ActiveWorkflow {
    pub fn new(id: impl ToString, step: usize) -> Self {
        Self {
            id: id.to_string(),
            step,
            memory: HashMap::new(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct TenonAgent {
    pub model: SupportedModels,
    pub directive: Vec<Directive>,
    pub tool_names: Vec<String>,
    pub workflows: Vec<WorkflowConfig>,
}

impl TenonAgent {
    pub fn new(
        model: SupportedModels,
        directive: Vec<Directive>,
        tools: &[impl AsRef<str>],
        workflows: Vec<WorkflowConfig>,
    ) -> Self {
        Self {
            model,
            directive,
            tool_names: tools.iter().map(|t| t.as_ref().to_string()).collect(),
            workflows,
        }
    }

    pub fn build_chat_adapter(
        &self,
        workflow_context: Arc<RwLock<Option<ActiveWorkflow>>>,
        log_indexer: Arc<RwLock<ChatLogIndexer>>,
    ) -> ChatAgent {
        // NOTE: Update token estimation when this prompt changes
        let mut system_prompt = "Running on Tenon. Output markdown. Be brief, No filler or hedging or unnecessary words. Reduce emoji use. \
            Files content changes anytime. File ≠ expected → never revert, re-read → re-understand → changes. \
            History shows active behavior/prompt at that time. Prior actions may span agents → trust reported behavior. \
            Earlier history may be truncated. Missing context → ask user for clarification.".to_string();

        // Add workflow information if agent has workflows configured
        if !self.workflows.is_empty() {
            let registry = get_workflow_registry();
            let workflow_info: Vec<String> = self
                .workflows
                .iter()
                .filter_map(|w| {
                    let condition = w
                        .condition
                        .as_ref()
                        .or_else(|| registry.get(&w.id).map(|wf| &wf.default_condition))?;
                    Some(format!(
                        "<workflow condition=\"{}\" id=\"{}\" />",
                        condition, w.id
                    ))
                })
                .collect();

            system_prompt.push_str(&format!(
                "\n\n<workflow />=multi-step process. Start: `start_workflow <id>` when not in workflow. Condition match → ALWAYS PRIORITIZE WORKFLOW over unguided steps.\n\
                {}",
                workflow_info.join("")
            ));
        }

        system_prompt.push_str(
            "\n\n<directive></directive>=rules for agent conduct; no condition→always, condition→when matched. \
            Explicit user instruction overrides directives.");

        let mut combined = vec![Directive {
            condition: None,
            source: DirectiveSource::Text {
                value: system_prompt,
            },
        }];
        combined.extend(self.directive.iter().cloned());

        let mut tools = resolve_tools(&self.tool_names);

        // Add start_workflow tool if agent has workflows configured (and no active workflow)
        if !self.workflows.is_empty() {
            let has_active = workflow_context
                .read()
                .map(|g| g.is_some())
                .unwrap_or(false);

            if !has_active {
                use crate::tools::start_workflow::StartWorkflow;
                tools.push(Box::new(StartWorkflow {
                    workflow_ids: self.workflows.iter().map(|w| w.id.clone()).collect(),
                    active_workflow: workflow_context.clone(),
                    log_indexer: log_indexer.clone(),
                }));
            }
        }

        // Add workflow navigation tool if there's an active workflow
        let has_active = {
            if let Ok(active_read) = workflow_context.read() {
                active_read.is_some()
            } else {
                false
            }
        };
        if has_active {
            use crate::tools::end_workflow::EndWorkflow;
            use crate::tools::navigate_workflow::NavigateWorkflow;
            tools.push(Box::new(NavigateWorkflow {
                active_workflow: workflow_context.clone(),
                log_indexer: log_indexer.clone(),
            }));
            tools.push(Box::new(EndWorkflow {
                active_workflow: workflow_context,
                log_indexer,
            }));
        }

        get_agent(self.model.clone(), combined, tools)
    }
}

pub struct ChatSession {
    pub id: String,
    pub title: Arc<RwLock<Option<String>>>,
    pub log_indexer: Arc<RwLock<ChatLogIndexer>>,
    pub usage: Arc<RwLock<Option<Usage>>>,
    pub active_agent: ActiveAgent,
    pub active_workflow: Arc<RwLock<Option<ActiveWorkflow>>>,
    pub session_datetime: DateTime<Local>,
    cancel_token: Arc<AtomicBool>,
    active_thread: Option<std::thread::JoinHandle<()>>,
    cancel_title_token: Arc<AtomicBool>,
    title_thread: Option<std::thread::JoinHandle<()>>,
}

impl ChatSession {
    pub fn new() -> Self {
        Self::with_agent_name(get_application_config().default_agent)
            .expect("the program failed to enforce default_agent validation")
    }

    pub fn with_agent_name(agent_name: String) -> OxiResult<Self> {
        Ok(Self {
            id: generate_chat_id(),
            title: Arc::new(RwLock::new(None)),
            log_indexer: Arc::new(RwLock::new(ChatLogIndexer::new())),
            usage: Arc::new(RwLock::new(None)),
            active_agent: ActiveAgent {
                name: agent_name.to_string(),
                inner: get_application_config()
                    .agents
                    .get(&agent_name)
                    .ok_or(nvim_oxi::Error::Mlua(mlua::Error::RuntimeError("".into())))?
                    .clone(),
            },
            active_workflow: Arc::new(RwLock::new(None)),
            session_datetime: Local::now(),
            cancel_token: Arc::new(AtomicBool::new(false)),
            active_thread: None,
            cancel_title_token: Arc::new(AtomicBool::new(false)),
            title_thread: None,
        })
    }

    pub fn from_history(history: history::ChatHistory) -> OxiResult<Self> {
        let config = get_application_config();
        let (agent_name, agent) = config
            .agents
            .get(&history.agent_name)
            .map(|a| (history.agent_name.clone(), a.clone()))
            .or_else(|| {
                config
                    .agents
                    .get(&config.default_agent)
                    .map(|a| (config.default_agent.clone(), a.clone()))
            })
            .ok_or_else(|| {
                nvim_oxi::Error::Mlua(mlua::Error::RuntimeError(
                    "no agent found in config".to_string(),
                ))
            })?;

        let logs: Vec<TenonLog> = history.logs;

        // Replay workflow logs to reconstruct active_workflow state.
        // Active workflow is derived from history, not stored directly,
        // because it's constructed from logic (step progression/end), not raw state.
        let active_workflow: Option<ActiveWorkflow> = {
            let mut wf: Option<ActiveWorkflow> = None;
            for log in &logs {
                if let TenonLogData::Workflow(wf_log) = log.data() {
                    match wf_log.step {
                        Some(step) => {
                            // Navigate/create: set workflow to this step
                            wf = Some(ActiveWorkflow::new(&wf_log.id, step));
                        }
                        None => {
                            // End: clear active workflow
                            wf = None;
                        }
                    }
                }
            }
            wf
        };

        let log_indexer = ChatLogIndexer::from_logs(logs);

        let session = Self {
            id: history.id,
            title: Arc::new(RwLock::new(history.title)),
            log_indexer: Arc::new(RwLock::new(log_indexer)),
            usage: Arc::new(RwLock::new(history.usage)),
            active_agent: ActiveAgent {
                name: agent_name,
                inner: agent,
            },
            active_workflow: Arc::new(RwLock::new(active_workflow)),
            session_datetime: history.session_datetime,
            cancel_token: Arc::new(AtomicBool::new(false)),
            active_thread: None,
            cancel_title_token: Arc::new(AtomicBool::new(false)),
            title_thread: None,
        };

        Ok(session)
    }

    pub fn cancel(&mut self) {
        self.cancel_token.store(true, Ordering::SeqCst);
    }

    pub fn cancel_title(&mut self) {
        self.cancel_title_token.store(true, Ordering::SeqCst);
    }

    pub fn is_processing(&self) -> bool {
        let main_thread_running = if let Some(thread) = self.active_thread.as_ref() {
            !thread.is_finished()
        } else {
            false
        };

        let title_thread_running = if let Some(thread) = self.title_thread.as_ref() {
            !thread.is_finished()
        } else {
            false
        };

        main_thread_running || title_thread_running
    }

    /// Generates a title for the chat if not already set.
    /// Runs in a separate thread to avoid blocking the main chat stream.
    pub fn generate_title(&mut self, first_message: String) {
        if self.title.read().map(|t| t.is_some()).unwrap_or(false) {
            return;
        }

        // Cancel previous title generation
        self.cancel_title_token.store(true, Ordering::SeqCst);
        self.cancel_title_token = Arc::new(AtomicBool::new(false));
        let cancel_token = Arc::clone(&self.cancel_title_token);

        let title_arc = Arc::clone(&self.title);
        let config = get_application_config();

        self.title_thread = Some(std::thread::spawn(move || {
            let rt = tokio::runtime::Runtime::new().unwrap();
            rt.block_on(async {
                // Get title model or fall back to default agent's model
                let model = config.title.model.clone().or_else(|| {
                    config
                        .agents
                        .get(&config.default_agent)
                        .map(|a| a.model.clone())
                });

                let model = match model {
                    Some(m) => m,
                    None => return,
                };

                let directive = vec![Directive {
                    condition: None,
                    source: DirectiveSource::Text {
                        value: config.title.prompt.clone(),
                    },
                }];

                let agent = get_agent(model, directive, vec![]);

                match agent
                    .chat(format!("Generate title:\n```\n{}\n```", first_message))
                    .await
                {
                    Ok(title) => {
                        if cancel_token.load(Ordering::SeqCst) {
                            return;
                        }
                        let trimmed = title.trim();
                        if !trimmed.is_empty()
                            && let Ok(mut t) = title_arc.write()
                        {
                            *t = Some(
                                trimmed
                                    .lines()
                                    .collect::<Vec<_>>()
                                    .first()
                                    .map(|x| x.to_string())
                                    .unwrap_or("Untitled".to_string()),
                            );
                        }
                    }
                    Err(e) => {
                        eprintln!("[tenon] Failed to generate title: {}", e);
                    }
                }
            });
        }));
    }

    pub fn is_generating_title(&self) -> bool {
        if let Some(thread) = self.title_thread.as_ref() {
            !thread.is_finished()
        } else {
            false
        }
    }

    /// Internal method to send a chat request.
    /// If save_prompt is true, it will be added to logs before sending.
    /// RAG context is computed inside the spawned thread to avoid blocking.
    fn send_chat_request(&mut self, prompt: String, save_prompt: bool) {
        // Cancel previous thread
        self.cancel_token.store(true, Ordering::SeqCst);
        self.cancel_token = Arc::new(AtomicBool::new(false));

        // Generate title if this is a new user message
        if save_prompt {
            self.generate_title(prompt.clone());
        }

        self.prune_incomplete_messages();

        let log_indexer_clone = self.log_indexer.clone();
        let usage_clone = Arc::clone(&self.usage);
        let agent_clone = self.active_agent.clone();
        let chat_id = self.id.clone();
        let title_clone = Arc::clone(&self.title);
        let session_datetime = self.session_datetime;
        let cancel_token = Arc::clone(&self.cancel_token);
        let active_workflow_clone = Arc::clone(&self.active_workflow);

        self.active_thread = Some(std::thread::spawn(move || {
            let rt = tokio::runtime::Runtime::new().unwrap();

            rt.block_on(async {
                let mut next_prompt = prompt;
                let mut save_next_prompt = save_prompt;
                loop {
                    let mut should_continue = false;
                    let agent = agent_clone.build_chat_adapter(
                        active_workflow_clone.clone(),
                        log_indexer_clone.clone(),
                    );

                    // Add user message if provided
                    if save_next_prompt && let Ok(mut indexer) = log_indexer_clone.write() {
                        indexer.logs.push(crate::chat::log_indexer::IndexedLog {
                            log: Arc::new(TenonLog::new(TenonLogData::User(
                                TenonUserMessage::Text(TenonUserTextMessage(next_prompt.clone())),
                            ))),
                            active: true,
                        });
                        save_next_prompt = false;
                    }

                    let chat_history = if let Ok(mut indexer) = log_indexer_clone.write() {
                        indexer.retrieve_chatlog_with_context(&next_prompt)
                    } else {
                        Vec::new()
                    };

                    let prompt = build_workflow_prompt(&active_workflow_clone, next_prompt.clone());
                    let mut stream = agent
                        .stream_chat(prompt.clone(), chat_history.clone())
                        .await;

                    while let Some(result) = stream.next().await {
                        if cancel_token.load(Ordering::SeqCst) {
                            break;
                        }
                        match result {
                            Ok(StreamItem::ToolResult {
                                tool_result,
                                internal_call_id,
                            }) => {
                                if let Ok(mut indexer) = log_indexer_clone.write()
                                    && let Some(log) = indexer.logs.iter_mut().find_map(|x| {
                                        if let TenonLogData::Tool(tool) = x.log.data()
                                            && tool.tool_call.internal_call_id == internal_call_id
                                        {
                                            return Some(x);
                                        }
                                        None
                                    })
                                {
                                    let log = Arc::make_mut(&mut log.log);
                                    let tool_result = tool_result.content.first();
                                    let result = match tool_result {
                                        ToolResultContent::Text(text) => {
                                            if text.text.starts_with("Toolset error: ") {
                                                Err(TenonToolError(text.text))
                                            } else {
                                                Ok(TenonToolResult::Text(text))
                                            }
                                        }
                                        ToolResultContent::Image(img) => {
                                            Ok(TenonToolResult::Image(img))
                                        }
                                    };

                                    log.set_tool_result(Some(result.clone()));

                                    // Handle workflow tool results
                                    if let TenonLogData::Tool(tool_log) = log.data() {
                                        match tool_log.tool_call.name.as_str() {
                                            "start_workflow" if result.is_ok() => {
                                                // Continue to first workflow step
                                                should_continue = true;
                                                next_prompt = "[continue]".to_string();
                                                break;
                                            }
                                            "navigate_workflow" if result.is_ok() => {
                                                // Extract step_output for continuation
                                                if let Some(args_obj) =
                                                    tool_log.tool_call.args.as_object()
                                                    && let Some(serde_json::Value::String(
                                                        step_output,
                                                    )) = args_obj.get("step_output")
                                                {
                                                    should_continue = true;
                                                    next_prompt = format!(
                                                        "The previous step output: {}",
                                                        step_output
                                                    );
                                                    break;
                                                }
                                            }
                                            "end_workflow" if result.is_ok() => {
                                                // Extract output for continuation
                                                if let Some(args_obj) =
                                                    tool_log.tool_call.args.as_object()
                                                    && let Some(serde_json::Value::String(output)) =
                                                        args_obj.get("output")
                                                {
                                                    should_continue = true;
                                                    next_prompt = format!(
                                                        "Workflow ended with output: {}",
                                                        output
                                                    );
                                                    break;
                                                }
                                            }
                                            _ => {}
                                        }
                                    }
                                }
                            }
                            Ok(StreamItem::ReasoningDelta { reasoning }) => {
                                if let Ok(mut indexer) = log_indexer_clone.write() {
                                    let mut updated = false;
                                    if let Some(indexed_log) = indexer.logs.last_mut() {
                                        let log = Arc::make_mut(&mut indexed_log.log);
                                        updated = log.append_reasoning(&reasoning);
                                    }

                                    if !updated {
                                        indexer.logs.push(crate::chat::log_indexer::IndexedLog {
                                            log: Arc::new(TenonLog::new(TenonLogData::Assistant(
                                                TenonAssistantMessage {
                                                    reasoning: Some(reasoning),
                                                    content: vec![],
                                                },
                                            ))),
                                            active: true,
                                        });
                                    }
                                }
                            }
                            Ok(StreamItem::Text { text }) => {
                                if let Ok(mut indexer) = log_indexer_clone.write() {
                                    let mut updated = false;
                                    if let Some(indexed_log) = indexer.logs.last_mut() {
                                        let log = Arc::make_mut(&mut indexed_log.log);
                                        updated = log.append_text(&text);
                                    }

                                    if !updated {
                                        indexer.logs.push(crate::chat::log_indexer::IndexedLog {
                                            log: Arc::new(TenonLog::new(TenonLogData::Assistant(
                                                TenonAssistantMessage {
                                                    reasoning: None,
                                                    content: vec![
                                                        TenonAssistantMessageContent::Text(text),
                                                    ],
                                                },
                                            ))),
                                            active: true,
                                        });
                                    }
                                }
                            }
                            Ok(StreamItem::ToolCall {
                                tool_call,
                                internal_call_id,
                            }) => {
                                if let Ok(mut indexer) = log_indexer_clone.write() {
                                    indexer.logs.push(crate::chat::log_indexer::IndexedLog {
                                        log: Arc::new(TenonLog::new(TenonLogData::Tool(
                                            TenonToolLog {
                                                tool_call: TenonToolCall {
                                                    id: tool_call.id,
                                                    internal_call_id,
                                                    name: tool_call.function.name,
                                                    args: tool_call.function.arguments,
                                                },
                                                tool_result: None,
                                            },
                                        ))),
                                        active: true,
                                    });
                                }
                            }
                            Ok(StreamItem::Final { token_usage }) => {
                                if let Some(usage) = token_usage
                                    && let Ok(mut usage_lock) = usage_clone.write()
                                {
                                    *usage_lock = Some(usage);
                                }
                                let history_dir = get_application_config().history.directory;
                                let title_val = title_clone.read().ok().and_then(|t| t.clone());
                                if let Ok(indexer) = log_indexer_clone.read() {
                                    save_to_history(
                                        history::SessionMetadata {
                                            id: &chat_id,
                                            title: title_val.as_deref(),
                                            agent_name: &agent_clone.name,
                                            model_display: &agent_clone.inner.model.display_name(),
                                            session_datetime,
                                        },
                                        &indexer,
                                        &usage_clone,
                                        &history_dir,
                                    );
                                }
                            }
                            Ok(StreamItem::Other) => {}
                            Err(e) => {
                                GLOBAL_EXECUTION_HANDLER.notify_on_main_thread(
                                    format!(
                                        "error occurred while streaming response from LLM: {}",
                                        e
                                    ),
                                    LogLevel::Error,
                                );
                            }
                        }
                    }

                    if !should_continue || cancel_token.load(Ordering::SeqCst) {
                        break;
                    }
                }
            });
        }));
    }

    /// Continue the chat without adding a new user message.
    /// Useful for prompting the LLM to continue from where it left off.
    pub fn continue_chat(&mut self) {
        self.send_chat_request("[continue]".to_string(), false);
    }

    pub fn send_message(&mut self, message: String) {
        self.send_chat_request(message.clone(), true);
    }

    /// Prunes trailing incomplete messages (e.g., tool calls without results)
    /// from the session logs to prevent sending broken history to the LLM.
    pub fn prune_incomplete_messages(&self) {
        let Ok(mut indexer) = self.log_indexer.write() else {
            return;
        };

        let logs = &indexer.logs;
        let last_non_tool_index = logs
            .iter()
            .enumerate()
            .rfind(|(_, log)| !matches!(log.log.data(), TenonLogData::Tool(_)));

        if let Some((index, _)) = last_non_tool_index {
            let mut new_logs = Vec::with_capacity(logs.len());
            new_logs.extend_from_slice(&logs[..=index]);

            for log in &logs[index + 1..] {
                if let TenonLogData::Tool(tool_log) = log.log.data()
                    && tool_log.tool_result.is_some()
                {
                    new_logs.push(log.clone());
                }
            }
            indexer.logs = new_logs;
        } else {
            // If all messages are tools, we only keep the ones with results
            indexer.logs = logs
                .iter()
                .filter(|log| {
                    if let TenonLogData::Tool(tool_log) = log.log.data() {
                        tool_log.tool_result.is_some()
                    } else {
                        true
                    }
                })
                .cloned()
                .collect();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chat::log::{
        TenonLog, TenonLogData, TenonToolCall, TenonToolLog, TenonUserMessage, TenonUserTextMessage,
    };
    use serde_json::json;
    use std::sync::Arc;

    fn create_user_log(text: &str) -> super::log_indexer::IndexedLog {
        super::log_indexer::IndexedLog {
            log: Arc::new(TenonLog::new(TenonLogData::User(TenonUserMessage::Text(
                TenonUserTextMessage(text.to_string()),
            )))),
            active: true,
        }
    }

    fn create_tool_log(name: &str, result: bool) -> super::log_indexer::IndexedLog {
        let tool_call = TenonToolCall {
            id: "1".into(),
            internal_call_id: "1".into(),
            name: name.into(),
            args: json!({}),
        };
        let tool_result = if result {
            Some(Ok(TenonToolResult::Text(rig::agent::Text {
                text: "ok".into(),
            })))
        } else {
            None
        };
        super::log_indexer::IndexedLog {
            log: Arc::new(TenonLog::new(TenonLogData::Tool(TenonToolLog {
                tool_call,
                tool_result,
            }))),
            active: true,
        }
    }

    #[test]
    fn test_prune_incomplete_messages() {
        let session = ChatSession::new();
        {
            let mut indexer = session.log_indexer.write().unwrap();
            indexer.logs = vec![
                create_user_log("Hello"),
                create_tool_log("tool1", false), // Incomplete
                create_tool_log("tool2", true),  // Complete
                create_tool_log("tool3", false), // Incomplete
            ];
        }

        session.prune_incomplete_messages();

        let indexer = session.log_indexer.read().unwrap();
        assert_eq!(indexer.logs.len(), 2);
        assert!(matches!(indexer.logs[0].log.data(), TenonLogData::User(_)));
        assert!(matches!(indexer.logs[1].log.data(), TenonLogData::Tool(_)));
        if let TenonLogData::Tool(tl) = &indexer.logs[1].log.data() {
            assert!(tl.tool_result.is_some());
        }
    }

    #[test]
    fn test_active_workflow_memory() {
        let workflow = ActiveWorkflow::new("test_workflow", 1);
        assert!(workflow.memory.is_empty());

        // Verify memory field exists and can store values
        let mut workflow = ActiveWorkflow::new("test_workflow", 1);
        workflow
            .memory
            .insert("key1".to_string(), "value1".to_string());
        workflow
            .memory
            .insert("key2".to_string(), "value2".to_string());

        assert_eq!(workflow.memory.get("key1"), Some(&"value1".to_string()));
        assert_eq!(workflow.memory.get("key2"), Some(&"value2".to_string()));
        assert_eq!(workflow.memory.len(), 2);
    }

    #[test]
    fn test_build_workflow_prompt_displays_memory() {
        // Test that build_workflow_prompt includes stored memory in context
        // Initialize PLUGIN_ROOT for testing
        crate::utils::PLUGIN_ROOT
            .set(std::env::current_dir().unwrap())
            .ok();

        let workflow = Arc::new(RwLock::new(Some(ActiveWorkflow {
            id: "implement_software".to_string(),
            step: 1,
            memory: {
                let mut m = HashMap::new();
                m.insert("previous_output".to_string(), "test result".to_string());
                m
            },
        })));

        let prompt = build_workflow_prompt(&workflow, "user input".to_string());

        // Memory should be included in the prompt
        assert!(prompt.contains("<memory name=\"previous_output\">"));
        assert!(prompt.contains("test result"));
        assert!(prompt.contains("</memory>"));
    }
}
