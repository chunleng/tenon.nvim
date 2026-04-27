use crate::{
    clients::{BehaviorSource, ChatAgent, StreamItem, SupportedModels, get_agent},
    get_application_config,
    tools::resolve_tools,
    utils::GLOBAL_EXECUTION_HANDLER,
};
use chrono::Local;
use nvim_oxi::{Result as OxiResult, api::types::LogLevel};
use rig::{
    completion::Usage,
    message::{Message, ToolResultContent},
    tool::ToolDyn,
};
use std::{
    collections::LinkedList,
    sync::atomic::{AtomicBool, Ordering},
    sync::{Arc, LazyLock, Mutex, RwLock},
};

pub mod history;
pub mod log;

pub use log::{
    TenonAssistantMessage, TenonAssistantMessageContent, TenonLog, TenonToolCall, TenonToolError,
    TenonToolLog, TenonToolResult, TenonUserMessage, TenonUserTextMessage,
};

use history::save_to_history;

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

pub struct ChatSession {
    pub id: String,
    pub title: Arc<RwLock<Option<String>>>,
    pub logs: Arc<RwLock<LinkedList<TenonLog>>>,
    pub usage: Arc<RwLock<Option<Usage>>>,
    pub active_agent: ActiveAgent,
    cancel_token: Arc<AtomicBool>,
    active_thread: Option<std::thread::JoinHandle<()>>,
    cancel_title_token: Arc<AtomicBool>,
    title_thread: Option<std::thread::JoinHandle<()>>,
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
pub struct TenonAgent {
    pub model: SupportedModels,
    pub behavior: Vec<BehaviorSource>,
    pub tool_names: Vec<String>,
}

const SYSTEM_BEHAVIOR: &str = "Output markdown. Concise, not verbose. No filler or hedging or unnecessary words. Reduce emoji use. \
    User may edit files between steps → files change silently. File state ≠ before? → user likely edited. Think why. \
    Chat history may span agents with different tools/behavior. Prior assistant actions ≠ yours.";

impl TenonAgent {
    pub fn new(
        model: SupportedModels,
        behavior: Vec<BehaviorSource>,
        tools: &[impl AsRef<str>],
    ) -> Self {
        Self {
            model,
            behavior,
            tool_names: tools.iter().map(|t| t.as_ref().to_string()).collect(),
        }
    }

    pub fn build_chat_adapter(&self, tools: Vec<Box<dyn ToolDyn>>) -> ChatAgent {
        let mut combined = vec![BehaviorSource::Text {
            value: SYSTEM_BEHAVIOR.to_string(),
        }];
        combined.extend(self.behavior.iter().cloned());
        get_agent(self.model.clone(), combined, tools)
    }
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
            logs: Arc::new(RwLock::new(LinkedList::new())),
            usage: Arc::new(RwLock::new(None)),
            active_agent: ActiveAgent {
                name: agent_name.to_string(),
                inner: get_application_config()
                    .agents
                    .get(&agent_name)
                    .ok_or(nvim_oxi::Error::Mlua(mlua::Error::RuntimeError("".into())))?
                    .clone(),
            },
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

        let logs: LinkedList<TenonLog> = history.logs.into_iter().collect();

        Ok(Self {
            id: history.id,
            title: Arc::new(RwLock::new(history.title)),
            logs: Arc::new(RwLock::new(logs)),
            usage: Arc::new(RwLock::new(history.usage)),
            active_agent: ActiveAgent {
                name: agent_name,
                inner: agent,
            },
            cancel_token: Arc::new(AtomicBool::new(false)),
            active_thread: None,
            cancel_title_token: Arc::new(AtomicBool::new(false)),
            title_thread: None,
        })
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

                let behavior = vec![BehaviorSource::Text {
                    value: config.title.prompt.clone(),
                }];

                let agent = get_agent(model, behavior, vec![]);

                match agent
                    .chat(format!("Generate title:\n```\n{}\n```", first_message))
                    .await
                {
                    Ok(title) => {
                        if cancel_token.load(Ordering::SeqCst) {
                            return;
                        }
                        let trimmed = title.trim();
                        if !trimmed.is_empty() {
                            if let Ok(mut t) = title_arc.write() {
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

    pub fn send_message(&mut self, message: String) {
        // Cancel previous thread
        self.cancel_token.store(true, Ordering::SeqCst);

        // Create new cancel token for the new thread
        self.cancel_token = Arc::new(AtomicBool::new(false));
        let cancel_token = Arc::clone(&self.cancel_token);

        // Generate title if not already set
        self.generate_title(message.clone());

        let logs_clone = Arc::clone(&self.logs);
        let usage_clone = Arc::clone(&self.usage);
        let agent_clone = self.active_agent.clone();
        let chat_id = self.id.clone();
        let title_clone = Arc::clone(&self.title);

        self.active_thread = Some(std::thread::spawn(move || {
            let rt = tokio::runtime::Runtime::new().unwrap();
            rt.block_on(async {
                let tools = resolve_tools(&agent_clone.tool_names);
                let agent = agent_clone.build_chat_adapter(tools);
                let chat_history;
                if let Ok(logs) = logs_clone.read() {
                    chat_history = logs
                        .iter()
                        .cloned()
                        .flat_map(|x| Vec::<Message>::from(x))
                        .collect::<Vec<_>>();
                } else {
                    todo!("fix after error is introduced")
                }

                if let Ok(mut logs) = logs_clone.write() {
                    logs.push_back(TenonLog::User(TenonUserMessage::Text(
                        TenonUserTextMessage(message.clone()),
                    )))
                }

                let mut stream = agent.stream_chat(message, chat_history).await;
                while let Some(result) = stream.next().await {
                    if cancel_token.load(Ordering::SeqCst) {
                        break;
                    }
                    match result {
                        Ok(StreamItem::ToolResult {
                            tool_result,
                            internal_call_id,
                        }) => {
                            if let Ok(mut logs) = logs_clone.write() {
                                if let Some(log) = logs.iter_mut().find_map(|x| match x {
                                    TenonLog::Tool(tool)
                                        if tool.tool_call.internal_call_id == internal_call_id =>
                                    {
                                        Some(tool)
                                    }
                                    _ => None,
                                }) {
                                    let tool_result = tool_result.content.first();
                                    log.tool_result = Some(match tool_result {
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
                                    });
                                }
                            }
                        }
                        Ok(StreamItem::ReasoningDelta { reasoning }) => {
                            if let Ok(mut logs) = logs_clone.write() {
                                let mut updated = false;
                                if let Some(log) = logs.back_mut()
                                    && let TenonLog::Assistant(TenonAssistantMessage {
                                        reasoning: text,
                                        ..
                                    }) = log
                                {
                                    match text {
                                        Some(x) => {
                                            x.push_str(&reasoning);
                                        }
                                        None => *text = Some(reasoning.clone()),
                                    }
                                    updated = true;
                                }

                                if !updated {
                                    logs.push_back(TenonLog::Assistant(TenonAssistantMessage {
                                        reasoning: Some(reasoning),
                                        content: vec![],
                                    }));
                                }
                            }
                        }
                        Ok(StreamItem::Text { text }) => {
                            if let Ok(mut logs) = logs_clone.write() {
                                let mut updated = false;
                                if let Some(log) = logs.back_mut()
                                    && let TenonLog::Assistant(TenonAssistantMessage {
                                        content,
                                        ..
                                    }) = log
                                {
                                    if let Some(TenonAssistantMessageContent::Text(s)) =
                                        content.last_mut()
                                    {
                                        s.push_str(&text);
                                        updated = true;
                                    } else {
                                        content
                                            .push(TenonAssistantMessageContent::Text(text.clone()));
                                        updated = true
                                    }
                                }

                                if !updated {
                                    logs.push_back(TenonLog::Assistant(TenonAssistantMessage {
                                        reasoning: None,
                                        content: vec![TenonAssistantMessageContent::Text(text)],
                                    }));
                                }
                            }
                        }
                        Ok(StreamItem::ToolCall {
                            tool_call,
                            internal_call_id,
                        }) => {
                            if let Ok(mut logs) = logs_clone.write() {
                                logs.push_back(TenonLog::Tool(TenonToolLog {
                                    tool_call: TenonToolCall {
                                        id: tool_call.id,
                                        internal_call_id: internal_call_id,
                                        name: tool_call.function.name,
                                        args: tool_call.function.arguments,
                                    },
                                    tool_result: None,
                                }));
                            }
                        }
                        Ok(StreamItem::Final { token_usage }) => {
                            if let Some(usage) = token_usage {
                                if let Ok(mut usage_lock) = usage_clone.write() {
                                    *usage_lock = Some(usage);
                                }
                            }
                            let history_dir = get_application_config().history.directory;
                            let title_val = title_clone.read().ok().and_then(|t| t.clone());
                            save_to_history(
                                &chat_id,
                                title_val.as_deref(),
                                &agent_clone.name,
                                &agent_clone.inner.model.display_name(),
                                &logs_clone,
                                &usage_clone,
                                &history_dir,
                            );
                        }
                        Ok(StreamItem::Other) => {}
                        Err(e) => {
                            // TODO add tracing logs
                            let _ = GLOBAL_EXECUTION_HANDLER.notify_on_main_thread(
                                format!("error occurred while streaming response from LLM: {}", e),
                                LogLevel::Error,
                            );
                        }
                    }
                }
            });
        }));
    }
}
