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
use rig::{
    OneOrMany,
    completion::Usage,
    message::{Message, ToolResultContent, UserContent},
};
use std::{
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

            return format!(
                "<context>\n\
                    Currently in {} step of {} workflow. In a workflow, user prompt first, workflow instruction second and chat history is just reference\n\
                    When processing this input, prioritize: User message > <context>\n\
                    Follow through the process of the workflow step by step. Following is instruction of current step:\n\
                    <instruction>\n\
                    {}\n\
                    </instruction>\n\
                    After all process instruction has been completed, call navigate_workflow tool with the appropriate step number and your step_output.\n\
                    <navigation>\n\
                    {}\n\
                    </navigation>\n\
                    </context>\n\
                    {}",
                step.title,
                workflow.title,
                step.instruction.resolve().unwrap_or_default(),
                goto_instruction,
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
    pub id: String,
    pub step: usize,
}

impl ActiveWorkflow {
    pub fn new(id: impl ToString, step: usize) -> Self {
        Self {
            id: id.to_string(),
            step,
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
            Earlier history may be truncated. Missing context → ask user for clarification. \
            <directive></directive>=rules for agent conduct; no condition→always, condition→when matched. \
            Explicit user instruction overrides directives.".to_string();

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
                " <workflow />=structures that help you solve problems, call `start_workflow <id>` to start them. if workflow matches condition, always prioritize workflow over manually figuring out a process\n\
                In workflow + question for user → ask directly. Never via navigate/end_workflow.\n\
                {}",
                workflow_info.join("")
            ));
        }

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

        let logs: Vec<TenonLog> = history
            .logs
            .into_iter()
            .map(|mut log| {
                log.recount_tokens();
                log
            })
            .collect();

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

        let mut log_indexer = ChatLogIndexer::from_logs(logs);
        log_indexer.recount_all_tokens();
        log_indexer.apply_context_truncation();

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
    /// If user_message is Some, it will be added to logs before sending.
    /// RAG context is computed inside the spawned thread to avoid blocking.
    fn send_chat_request(&mut self, prompt: String, user_message: Option<String>) {
        // Cancel previous thread
        self.cancel_token.store(true, Ordering::SeqCst);
        self.cancel_token = Arc::new(AtomicBool::new(false));

        // Generate title if this is a new user message
        if let Some(msg) = &user_message {
            self.generate_title(msg.clone());
        }

        // Apply context truncation if needed
        if let Ok(mut indexer) = self.log_indexer.write() {
            indexer.apply_context_truncation();
        }

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
                let mut prompt = build_workflow_prompt(&active_workflow_clone, prompt);
                // Clean up trailing tool calls without results
                if let Ok(mut indexer) = log_indexer_clone.write() {
                    let mut logs_vec: Vec<_> = indexer.logs.to_vec();
                    indexer.logs.clear();

                    // Find where trailing tools start (last non-tool or tool with result)
                    let trailing_start = logs_vec
                        .iter()
                        .rposition(|log| !matches!(log.data(), TenonLogData::Tool(_)))
                        .map(|i| i + 1)
                        .unwrap_or(0);

                    // Keep only tools with results in the trailing section
                    let trailing_tools: Vec<_> = logs_vec[trailing_start..]
                        .iter()
                        .filter(|&log| {
                            if let TenonLogData::Tool(tool_log) = log.data() {
                                tool_log.tool_result.is_some()
                            } else {
                                true
                            }
                        })
                        .cloned()
                        .collect();

                    logs_vec.truncate(trailing_start);
                    logs_vec.extend(trailing_tools);

                    for log in logs_vec {
                        indexer.logs.push(log);
                    }
                }

                // Build chat_history
                let mut chat_history: Vec<Message> = if let Ok(indexer) = log_indexer_clone.read() {
                    indexer
                        .active_log()
                        .into_iter()
                        .flat_map(|x| Vec::<Message>::from((*x).clone()))
                        .collect()
                } else {
                    Vec::new()
                };

                // Add user message if provided
                if let Some(ref msg) = user_message
                    && let Ok(mut indexer) = log_indexer_clone.write()
                {
                    indexer.logs.push(Arc::new(TenonLog::new(TenonLogData::User(
                        TenonUserMessage::Text(TenonUserTextMessage(msg.clone())),
                    ))));
                }

                // Build RAG context (inside thread to avoid blocking main)
                let rag_context = if let Ok(indexer) = log_indexer_clone.read() {
                    let inactive_logs = indexer.inactive_log();
                    user_message
                        .as_ref()
                        .and_then(|msg| indexer.rag_context.build_context(&inactive_logs, msg))
                } else {
                    None
                };

                // Inject RAG context if available
                if let Some(ctx) = rag_context {
                    chat_history.insert(
                        0,
                        Message::User {
                            content: OneOrMany::one(UserContent::text(format!(
                                "[Context from earlier conversation]\n{}",
                                ctx.trim()
                            ))),
                        },
                    );
                }

                loop {
                    let mut should_continue = false;
                    let mut next_prompt = String::new();
                    let agent = agent_clone.build_chat_adapter(
                        active_workflow_clone.clone(),
                        log_indexer_clone.clone(),
                    );

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
                                        if let TenonLogData::Tool(tool) = x.data()
                                            && tool.tool_call.internal_call_id == internal_call_id
                                        {
                                            return Some(x);
                                        }
                                        None
                                    })
                                {
                                    let log = Arc::make_mut(log);
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
                                    if let Some(log) = indexer.logs.last_mut() {
                                        let log = Arc::make_mut(log);
                                        updated = log.append_reasoning(&reasoning);
                                    }

                                    if !updated {
                                        indexer.logs.push(Arc::new(TenonLog::new(
                                            TenonLogData::Assistant(TenonAssistantMessage {
                                                reasoning: Some(reasoning),
                                                content: vec![],
                                            }),
                                        )));
                                    }
                                }
                            }
                            Ok(StreamItem::Text { text }) => {
                                if let Ok(mut indexer) = log_indexer_clone.write() {
                                    let mut updated = false;
                                    if let Some(log) = indexer.logs.last_mut() {
                                        let log = Arc::make_mut(log);
                                        updated = log.append_text(&text);
                                    }

                                    if !updated {
                                        indexer.logs.push(Arc::new(TenonLog::new(
                                            TenonLogData::Assistant(TenonAssistantMessage {
                                                reasoning: None,
                                                content: vec![TenonAssistantMessageContent::Text(
                                                    text,
                                                )],
                                            }),
                                        )));
                                    }
                                }
                            }
                            Ok(StreamItem::ToolCall {
                                tool_call,
                                internal_call_id,
                            }) => {
                                if let Ok(mut indexer) = log_indexer_clone.write() {
                                    indexer.logs.push(Arc::new(TenonLog::new(TenonLogData::Tool(
                                        TenonToolLog {
                                            tool_call: TenonToolCall {
                                                id: tool_call.id,
                                                internal_call_id,
                                                name: tool_call.function.name,
                                                args: tool_call.function.arguments,
                                            },
                                            tool_result: None,
                                        },
                                    ))));
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

                    prompt = build_workflow_prompt(&active_workflow_clone, next_prompt);

                    chat_history = if let Ok(indexer) = log_indexer_clone.read() {
                        indexer
                            .active_log()
                            .into_iter()
                            .flat_map(|x| Vec::<Message>::from((*x).clone()))
                            .collect()
                    } else {
                        vec![]
                    };
                }
            });
        }));
    }

    /// Continue the chat without adding a new user message.
    /// Useful for prompting the LLM to continue from where it left off.
    pub fn continue_chat(&mut self) {
        self.send_chat_request("[continue]".to_string(), None);
    }

    pub fn send_message(&mut self, message: String) {
        self.send_chat_request(message.clone(), Some(message));
    }
}
