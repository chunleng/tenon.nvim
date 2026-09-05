use crate::agent::engine::{AgenticAgentType, AgenticStreamEngine};
use crate::chat::helpers::TitleHandler;
use crate::chat::history::{SessionMetadata, save_to_history};
use crate::get_application_config;
use crate::tools::ask_question::QuestionResult;
use crate::tools::resolve_tools;
use chrono::{DateTime, Local};
use nvim_oxi::Result as OxiResult;
use rig::completion::Usage;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, LazyLock, Mutex, RwLock};

pub mod event_channel;

pub mod helpers;
pub mod history;
pub mod log;

pub mod choreo;
pub mod prompt;
pub mod usage;
pub mod work_queue;

pub use event_channel::EventChannel;
pub use log::handler::ChatLogHandler;
pub use log::{
    TenonAssistantMessage, TenonAssistantMessageContent, TenonChoreoLog, TenonLog, TenonLogData,
    TenonThoughtLog, TenonToolCall, TenonToolError, TenonToolLog, TenonToolResult,
    TenonUserMessage,
};
pub use usage::SessionUsage;
pub use work_queue::WorkQueue;

pub use crate::agent::worker::full::TenonAgent;

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
pub struct ActiveChoreo {
    pub choreo: Arc<crate::chat::choreo::Choreo>,
    pub r#move: usize,
    pub memory: HashMap<String, String>,
}

impl ActiveChoreo {
    pub fn new(choreo: Arc<crate::chat::choreo::Choreo>, move_number: usize) -> Self {
        Self {
            choreo,
            r#move: move_number,
            memory: HashMap::new(),
        }
    }
}

/// Pending actions that require user interaction, stored per chat session.
#[derive(Clone)]
pub enum PendingAction {
    Question {
        question: String,
        options: Vec<String>,
        response_tx: Arc<Mutex<Option<tokio::sync::oneshot::Sender<QuestionResult>>>>,
    },
}

pub struct ChatSession {
    pub id: String,
    pub usage: Arc<RwLock<SessionUsage>>,
    pub active_agent_name: String,
    pub engine: AgenticStreamEngine,
    pub session_datetime: DateTime<Local>,
    pub title_handler: TitleHandler,
    pub pending_actions_channel: Arc<EventChannel<PendingAction>>,
    cancel_token: Arc<AtomicBool>,
    active_thread: Option<std::thread::JoinHandle<()>>,
}

impl ChatSession {
    pub fn new() -> Self {
        Self::with_agent_name(get_application_config().default_agent)
            .expect("the program failed to enforce default_agent validation")
    }

    pub fn with_agent_name(agent_name: String) -> OxiResult<Self> {
        let agent = get_application_config()
            .agents
            .get(&agent_name)
            .ok_or(nvim_oxi::Error::Mlua(mlua::Error::RuntimeError("".into())))?
            .clone();
        let pending_actions_channel = Arc::new(EventChannel::new());
        let engine = AgenticStreamEngine::new(
            agent.model,
            agent.directive,
            resolve_tools(&agent.tool_names),
            agent.choreos,
            AgenticAgentType::Direct(Arc::downgrade(&pending_actions_channel)),
        );
        let log_window = engine.log_handler.log_window.clone();
        Ok(Self {
            id: generate_chat_id(),
            usage: Arc::new(RwLock::new(SessionUsage::default())),
            active_agent_name: agent_name.to_string(),
            engine,
            session_datetime: Local::now(),
            pending_actions_channel,
            cancel_token: Arc::new(AtomicBool::new(false)),
            active_thread: None,
            title_handler: TitleHandler::new(log_window),
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
        let pending_actions_channel = Arc::new(EventChannel::new());

        let mut engine = AgenticStreamEngine::new(
            agent.model,
            agent.directive,
            resolve_tools(&agent.tool_names),
            agent.choreos,
            AgenticAgentType::Direct(Arc::downgrade(&pending_actions_channel)),
        );
        engine.load(logs);
        if let Ok(mut queue) = engine.work_queue.write() {
            *queue = history.work_queue.clone();
        }
        let log_window = engine.log_handler.log_window.clone();

        let session = Self {
            id: history.id,
            usage: Arc::new(RwLock::new(SessionUsage {
                accumulated: history.usage,
                last_exchange: Usage::new(),
            })),
            active_agent_name: agent_name,
            engine,
            session_datetime: history.session_datetime,
            pending_actions_channel,
            cancel_token: Arc::new(AtomicBool::new(false)),
            active_thread: None,
            title_handler: TitleHandler::from_history(history.title, log_window),
        };

        Ok(session)
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
    /// The user message must be added to the log handler before calling this.
    /// RAG context is computed inside the spawned thread to avoid blocking.
    fn send_chat_request(&mut self, prompt: String) {
        // Cancel previous thread
        self.cancel_token.store(true, Ordering::SeqCst);
        self.cancel_token = Arc::new(AtomicBool::new(false));

        // Generate title for the chat
        self.title_handler.generate_title();

        let mut engine = self.engine.clone();
        let usage_clone = Arc::clone(&self.usage);
        let work_queue_clone = Arc::clone(&engine.work_queue);
        let agent_name = self.active_agent_name.clone();
        let model_display = self.engine.model.display_name();
        let chat_id = self.id.clone();
        let title_clone = Arc::clone(&self.title_handler.title);
        let session_datetime = self.session_datetime;
        let cancel_token = Arc::clone(&self.cancel_token);
        let log_window_clone = engine.log_handler.log_window.clone();

        self.active_thread = Some(std::thread::spawn(move || {
            let rt = tokio::runtime::Runtime::new().unwrap();

            rt.block_on(async {
                let on_completion_call = move |usage: Usage| {
                    if let Ok(mut usage_lock) = usage_clone.write() {
                        usage_lock.add(usage);
                    }
                    let history_dir = get_application_config().history.directory;
                    let title_val = title_clone.read().ok().and_then(|t| t.clone());
                    if let Ok(log_window) = log_window_clone.read() {
                        save_to_history(
                            SessionMetadata {
                                id: &chat_id,
                                title: title_val.as_deref(),
                                agent_name: &agent_name,
                                model_display: &model_display,
                                session_datetime,
                            },
                            &log_window,
                            &usage_clone,
                            &work_queue_clone,
                            &history_dir,
                        );
                    }
                };

                loop {
                    let should_continue = engine
                        .process_turn(prompt.clone(), &cancel_token, &on_completion_call, 100)
                        .await;

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
        let prompt = self.engine.log_handler.get_user_prompt();
        if let Ok(mut log_window) = self.engine.log_handler.log_window.write() {
            log_window.prune_incomplete_messages();
        }
        self.send_chat_request(prompt);
    }

    pub fn send_message(&mut self, message: String) {
        self.engine.log_handler.add_user_message(message.clone());
        self.send_chat_request(message);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_active_choreo_memory() {
        crate::utils::PLUGIN_ROOT
            .set(std::env::current_dir().unwrap())
            .ok();
        let registry = crate::get_choreo_registry();
        let choreo = registry.get("implement_code").unwrap().clone();
        let active = ActiveChoreo::new(choreo, 1);
        assert!(active.memory.is_empty());

        // Verify memory field exists and can store values
        let registry2 = crate::get_choreo_registry();
        let choreo2 = registry2.get("implement_code").unwrap().clone();
        let mut active = ActiveChoreo::new(choreo2, 1);
        active
            .memory
            .insert("key1".to_string(), "value1".to_string());
        active
            .memory
            .insert("key2".to_string(), "value2".to_string());

        assert_eq!(active.memory.get("key1"), Some(&"value1".to_string()));
        assert_eq!(active.memory.get("key2"), Some(&"value2".to_string()));
        assert_eq!(active.memory.len(), 2);
    }
}
