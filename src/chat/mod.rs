use crate::chat::helpers::TitleHandler;
use crate::chat::workflow::Workflow;
use crate::directive::directive_path;
use crate::{
    clients::{ChatAgent, StreamItem, SupportedModels, get_agent},
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

pub mod helpers;
pub mod history;
pub mod log;

pub mod usage;
pub mod workflow;

pub use log::handler::ChatLogHandler;
pub use log::window::LogWindow;
pub use log::{
    TenonAssistantMessage, TenonAssistantMessageContent, TenonLog, TenonLogData, TenonToolCall,
    TenonToolError, TenonToolLog, TenonToolResult, TenonUserMessage, TenonUserTextMessage,
    TenonWorkflowLog,
};
pub use usage::SessionUsage;

use history::save_to_history;

/// Builds a workflow-wrapped prompt if there's an active workflow.
fn build_workflow_prompt(
    active_workflow: &Arc<RwLock<Option<ActiveWorkflow>>>,
    base_prompt: String,
) -> String {
    if let Ok(active_lock) = active_workflow.read()
        && let Some(active) = active_lock.as_ref()
    {
        let workflow = &active.workflow;
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
                        None => format!("{}end_workflow", condition),
                        Some(step) if step > total_steps => {
                            format!("{}end_workflow", condition)
                        }
                        Some(step) => {
                            format!("{}navigate_workflow step:{}", condition, step)
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
                    goto_lines.push("end_workflow".to_string());
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
                    Currently in {} step of {} workflow.\n\
                    Complete \"Process\" section in `instruction` tag. \
                    Upon full completion, never halfway unless explicitly asked, \
                    follow \"Output\" section to create step output; if no \"Output\" section, send \"none\". Then call tool from `navigate` tag to navigate.\n\
                    \n\n\
                    <instruction>\n\
                    {}\n\
                    </instruction>\n\
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

    base_prompt
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
    pub workflow: Arc<crate::chat::workflow::Workflow>,
    pub step: usize,
    pub memory: HashMap<String, String>,
}

impl ActiveWorkflow {
    pub fn new(workflow: Arc<crate::chat::workflow::Workflow>, step: usize) -> Self {
        Self {
            workflow,
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
    pub workflows: Vec<Arc<Workflow>>,
}

impl TenonAgent {
    pub fn new(
        model: SupportedModels,
        directive: Vec<Directive>,
        tools: &[impl AsRef<str>],
        workflows: Vec<Arc<Workflow>>,
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
        log_window: Arc<RwLock<LogWindow>>,
    ) -> ChatAgent {
        let mut combined = vec![Directive {
            condition: None,
            source: DirectiveSource::File {
                paths: vec![directive_path("tenon_constitution.md")],
            },
        }];
        combined.extend(self.directive.iter().cloned());

        let mut tools = resolve_tools(&self.tool_names);

        let has_active = workflow_context
            .read()
            .map(|g| g.is_some())
            .unwrap_or(false);

        if has_active {
            use crate::tools::end_workflow::EndWorkflow;
            use crate::tools::navigate_workflow::NavigateWorkflow;
            tools.push(Box::new(NavigateWorkflow {
                active_workflow: workflow_context.clone(),
                log_window: log_window.clone(),
            }));
            tools.push(Box::new(EndWorkflow {
                active_workflow: workflow_context,
                log_window,
            }));
        } else if !self.workflows.is_empty() {
            use crate::tools::start_workflow::StartWorkflow;
            tools.push(Box::new(StartWorkflow {
                workflows: self.workflows.clone(),
                active_workflow: workflow_context.clone(),
                log_window: log_window.clone(),
            }));
        }

        get_agent(self.model.clone(), combined, tools, true)
    }
}

pub struct ChatSession {
    pub id: String,
    pub log_handler: ChatLogHandler,
    pub usage: Arc<RwLock<SessionUsage>>,
    pub active_agent: ActiveAgent,
    pub active_workflow: Arc<RwLock<Option<ActiveWorkflow>>>,
    pub session_datetime: DateTime<Local>,
    cancel_token: Arc<AtomicBool>,
    active_thread: Option<std::thread::JoinHandle<()>>,
    title_handler: TitleHandler,
}

impl ChatSession {
    pub fn new() -> Self {
        Self::with_agent_name(get_application_config().default_agent)
            .expect("the program failed to enforce default_agent validation")
    }

    pub fn with_agent_name(agent_name: String) -> OxiResult<Self> {
        let log_handler = ChatLogHandler::new();
        Ok(Self {
            id: generate_chat_id(),
            log_handler: log_handler.clone(),
            usage: Arc::new(RwLock::new(SessionUsage::default())),
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
            title_handler: TitleHandler::new(log_handler.log_window.clone()),
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
            let registry = get_workflow_registry();
            let mut wf: Option<ActiveWorkflow> = None;
            for log in &logs {
                if let TenonLogData::Workflow(wf_log) = log.data() {
                    match wf_log.step {
                        Some(step) => {
                            // Navigate/create: set workflow to this step
                            if let Some(workflow) = registry.get(&wf_log.id) {
                                wf = Some(ActiveWorkflow::new(workflow.clone(), step));
                            }
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

        let log_handler = ChatLogHandler::from_logs(logs);

        let session = Self {
            id: history.id,
            log_handler: log_handler.clone(),
            usage: Arc::new(RwLock::new(SessionUsage {
                accumulated: history.usage,
                last_exchange: Usage::new(),
            })),
            active_agent: ActiveAgent {
                name: agent_name,
                inner: agent,
            },
            active_workflow: Arc::new(RwLock::new(active_workflow)),
            session_datetime: history.session_datetime,
            cancel_token: Arc::new(AtomicBool::new(false)),
            active_thread: None,
            title_handler: TitleHandler::from_history(
                history.title,
                log_handler.log_window.clone(),
            ),
        };

        Ok(session)
    }

    pub fn title(&self) -> Option<String> {
        self.title_handler.title.read().ok().and_then(|t| t.clone())
    }

    pub fn set_title(&self, title: Option<String>) {
        if let Ok(mut t) = self.title_handler.title.write() {
            *t = title;
        }
    }

    pub fn cancel(&mut self) {
        self.cancel_token.store(true, Ordering::SeqCst);
    }

    pub fn is_processing(&self) -> bool {
        let main_thread_running = self
            .active_thread
            .as_ref()
            .is_some_and(|t| !t.is_finished());

        let title_thread_running = self.title_handler.is_generating();

        main_thread_running || title_thread_running
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
            self.title_handler.generate_title();
        }

        if let Ok(mut log_window) = self.log_handler.log_window.write() {
            log_window.prune_incomplete_messages();
        };

        let mut log_handler = self.log_handler.clone();
        let usage_clone = Arc::clone(&self.usage);
        let agent_clone = self.active_agent.clone();
        let chat_id = self.id.clone();
        let title_clone = Arc::clone(&self.title_handler.title);
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
                        log_handler.log_window.clone(),
                    );

                    let chat_history = log_handler.get_chat_history(&next_prompt);

                    // Add user message if provided
                    if save_next_prompt && let Ok(mut log_window) = log_handler.log_window.write() {
                        log_window.logs.push(crate::chat::log::indexer::IndexedLog {
                            log: Arc::new(TenonLog::new(TenonLogData::User(
                                TenonUserMessage::Text(TenonUserTextMessage(next_prompt.clone())),
                            ))),
                            active: true,
                        });
                        save_next_prompt = false;
                    }

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
                                if let Ok(mut log_window) = log_handler.log_window.write()
                                    && let Some(log) = log_window.logs.iter_mut().find_map(|x| {
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
                                                next_prompt = "".to_string();
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
                                if let Ok(mut log_window) = log_handler.log_window.write() {
                                    let mut updated = false;
                                    if let Some(indexed_log) = log_window.logs.last_mut() {
                                        let log = Arc::make_mut(&mut indexed_log.log);
                                        updated = log.append_reasoning(&reasoning);
                                    }

                                    if !updated {
                                        log_window.logs.push(
                                            crate::chat::log::indexer::IndexedLog {
                                                log: Arc::new(TenonLog::new(
                                                    TenonLogData::Assistant(
                                                        TenonAssistantMessage {
                                                            reasoning: Some(reasoning),
                                                            content: vec![],
                                                        },
                                                    ),
                                                )),
                                                active: true,
                                            },
                                        );
                                    }
                                }
                            }
                            Ok(StreamItem::Text { text }) => {
                                if let Ok(mut log_window) = log_handler.log_window.write() {
                                    let mut updated = false;
                                    if let Some(indexed_log) = log_window.logs.last_mut() {
                                        let log = Arc::make_mut(&mut indexed_log.log);
                                        updated = log.append_text(&text);
                                    }

                                    if !updated {
                                        log_window.logs.push(
                                            crate::chat::log::indexer::IndexedLog {
                                                log: Arc::new(TenonLog::new(
                                                    TenonLogData::Assistant(
                                                        TenonAssistantMessage {
                                                            reasoning: None,
                                                            content: vec![
                                                                TenonAssistantMessageContent::Text(
                                                                    text,
                                                                ),
                                                            ],
                                                        },
                                                    ),
                                                )),
                                                active: true,
                                            },
                                        );
                                    }
                                }
                            }
                            Ok(StreamItem::ToolCall {
                                tool_call,
                                internal_call_id,
                            }) => {
                                if let Ok(mut log_window) = log_handler.log_window.write() {
                                    log_window.logs.push(crate::chat::log::indexer::IndexedLog {
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
                                    usage_lock.add(usage);
                                }
                                let history_dir = get_application_config().history.directory;
                                let title_val = title_clone.read().ok().and_then(|t| t.clone());
                                if let Ok(log_window) = log_handler.log_window.read() {
                                    save_to_history(
                                        history::SessionMetadata {
                                            id: &chat_id,
                                            title: title_val.as_deref(),
                                            agent_name: &agent_clone.name,
                                            model_display: &agent_clone.inner.model.display_name(),
                                            session_datetime,
                                        },
                                        &log_window,
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
        self.send_chat_request("".to_string(), false);
    }

    pub fn send_message(&mut self, message: String) {
        self.send_chat_request(message.clone(), true);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_active_workflow_memory() {
        crate::utils::PLUGIN_ROOT
            .set(std::env::current_dir().unwrap())
            .ok();
        let registry = crate::get_workflow_registry();
        let wf = registry.get("implement_code").unwrap().clone();
        let workflow = ActiveWorkflow::new(wf, 1);
        assert!(workflow.memory.is_empty());

        // Verify memory field exists and can store values
        let registry2 = crate::get_workflow_registry();
        let wf2 = registry2.get("implement_code").unwrap().clone();
        let mut workflow = ActiveWorkflow::new(wf2, 1);
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

        let registry = crate::get_workflow_registry();
        let wf = registry.get("implement_code").unwrap().clone();

        let workflow = Arc::new(RwLock::new(Some(ActiveWorkflow {
            workflow: wf,
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

    #[test]
    fn test_build_workflow_prompt_no_workflows() {
        // Initialize PLUGIN_ROOT for testing
        crate::utils::PLUGIN_ROOT
            .set(std::env::current_dir().unwrap())
            .ok();

        // No active workflow - should return base_prompt without context
        let workflow = Arc::new(RwLock::new(None));
        let prompt = build_workflow_prompt(&workflow, "user input".to_string());
        assert_eq!(prompt, "user input");
        assert!(!prompt.contains("<context>"));
    }
}
