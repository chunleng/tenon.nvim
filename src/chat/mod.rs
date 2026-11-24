use crate::agent::engine::AgenticStreamEngine;
use crate::chat::helpers::TitleHandler;
use crate::chat::history::{SessionMetadata, save_to_history};
use crate::get_application_config;
use crate::hooks::run_needs_attention_hooks;
use crate::tools::ask_question::Question;
use crate::utils::approval::Approval;
use chrono::{DateTime, Local};
use nvim_oxi::Result as OxiResult;
use rig::completion::Usage;
use rig::tool::ToolContext;
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, LazyLock, Mutex, PoisonError, RwLock};

pub mod event_channel;

pub mod helpers;
pub mod history;
pub mod log;

pub mod choreo;
pub mod project_skills;
pub mod prompt;
pub mod usage;
pub mod work_queue;

pub use event_channel::EventChannel;
pub use log::handler::ChatLogHandler;
pub use log::{
    TenonAssistantMessage, TenonAssistantMessageContent, TenonChoreoLog, TenonImageLog, TenonLog,
    TenonLogData, TenonQuestionLog, TenonToolCall, TenonToolError, TenonToolLog, TenonToolResult,
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

    fn current_move(&self) -> Option<&choreo::Move> {
        self.r#move
            .checked_sub(1)
            .and_then(|index| self.choreo.moves.get(index))
    }
}

/// Pending actions that require user interaction, stored per chat session.
#[derive(Clone)]
pub enum PendingAction {
    Question(Question),
    Approval(Approval),
}

pub struct ChatSession {
    pub id: String,
    pub usage: Arc<RwLock<SessionUsage>>,
    pub active_agent_name: String,
    pub engine: AgenticStreamEngine,
    pub session_datetime: DateTime<Local>,
    pub title_handler: TitleHandler,
    pub pending_actions_channel: Arc<EventChannel<PendingAction>>,
    /// The session's work queue
    pub work_queue: Arc<RwLock<WorkQueue>>,
    /// Names of hooks active for this session; in-memory only, never persisted.
    pub active_hooks: Arc<RwLock<HashSet<String>>>,
    cancel_token: Arc<AtomicBool>,
    active_thread: Option<std::thread::JoinHandle<()>>,
}

impl ChatSession {
    /// Hooks marked `enabled` in config start active in a new session.
    fn default_active_hooks() -> HashSet<String> {
        get_application_config()
            .hooks
            .iter()
            .filter(|hook| hook.enabled)
            .map(|hook| hook.name.clone())
            .collect()
    }

    /// Fires the session's needs-attention hooks whenever an action is queued
    /// into `pending_actions_channel`, so hooks run even when no UI is watching
    /// this session. Captures Arc clones; a session reference would create a cycle.
    fn wire_needs_attention_callback(session: &Self) {
        let active_hooks = Arc::clone(&session.active_hooks);
        let title = Arc::clone(&session.title_handler.title);
        session
            .pending_actions_channel
            .set_on_new_front(Arc::new(move |_| {
                // A poisoned lock still holds valid data; recover and fire with it.
                let title = title
                    .read()
                    .unwrap_or_else(PoisonError::into_inner)
                    .clone()
                    .unwrap_or_default();
                let active = active_hooks.read().unwrap_or_else(PoisonError::into_inner);
                run_needs_attention_hooks(&get_application_config().hooks, &active, &title);
            }));
    }

    pub fn new() -> Self {
        Self::with_agent_name(get_application_config().default_agent)
            .expect("the program failed to enforce default_agent validation")
    }

    pub(crate) fn switch_agent(&mut self, name: String, agent: &TenonAgent) {
        self.active_agent_name = name;
        self.engine.switch_agent(agent);
    }

    pub fn with_agent_name(agent_name: String) -> OxiResult<Self> {
        let agent = get_application_config()
            .agents
            .get(&agent_name)
            .ok_or(nvim_oxi::Error::Mlua(mlua::Error::RuntimeError("".into())))?
            .clone();
        let pending_actions_channel = Arc::new(EventChannel::new());
        let work_queue = Arc::new(RwLock::new(WorkQueue::default()));
        let tool_context = ToolContext::new()
            .with_scope(Arc::new(Arc::downgrade(&pending_actions_channel)))
            .with_scope(work_queue.clone());
        let engine = AgenticStreamEngine::from_agent(&agent, tool_context);
        let log_window = engine.log_handler.log_window.clone();
        let session = Self {
            id: generate_chat_id(),
            usage: Arc::new(RwLock::new(SessionUsage::default())),
            active_agent_name: agent_name.to_string(),
            engine,
            session_datetime: Local::now(),
            pending_actions_channel,
            work_queue,
            active_hooks: Arc::new(RwLock::new(Self::default_active_hooks())),
            cancel_token: Arc::new(AtomicBool::new(false)),
            active_thread: None,
            title_handler: TitleHandler::new(log_window),
        };
        Self::wire_needs_attention_callback(&session);
        Ok(session)
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

        let work_queue = Arc::new(RwLock::new(history.work_queue.clone()));
        let tool_context = ToolContext::new()
            .with_scope(Arc::new(Arc::downgrade(&pending_actions_channel)))
            .with_scope(work_queue.clone());
        let mut engine = AgenticStreamEngine::from_agent(&agent, tool_context);
        engine.load(logs);
        let log_window = engine.log_handler.log_window.clone();

        let session = Self {
            id: history.id,
            usage: Arc::new(RwLock::new(SessionUsage {
                accumulated: history.usage,
                last_exchange: Usage::default(),
            })),
            active_agent_name: agent_name,
            engine,
            session_datetime: history.session_datetime,
            pending_actions_channel,
            work_queue,
            active_hooks: Arc::new(RwLock::new(Self::default_active_hooks())),
            cancel_token: Arc::new(AtomicBool::new(false)),
            active_thread: None,
            title_handler: TitleHandler::from_history(history.title, log_window),
        };

        Self::wire_needs_attention_callback(&session);

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
        let work_queue_clone = Arc::clone(&self.work_queue);
        let agent_name = self.active_agent_name.clone();
        let model_display = self.engine.model.display_name();
        let chat_id = self.id.clone();
        let title_clone = Arc::clone(&self.title_handler.title);
        let session_datetime = self.session_datetime;
        let cancel_token = Arc::clone(&self.cancel_token);
        let log_window_clone = engine.log_handler.log_window.clone();
        let active_hooks = Arc::clone(&self.active_hooks);
        let title_for_hooks = Arc::clone(&self.title_handler.title);

        self.active_thread = Some(std::thread::spawn(move || {
            let rt = tokio::runtime::Runtime::new().unwrap();

            rt.block_on(async {
                let on_completion_call = move |usage: Usage| {
                    if let Ok(mut usage_lock) = usage_clone.write() {
                        usage_lock.add(usage);
                    }
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
                        );
                    }
                };

                let mut prompt = prompt.clone();
                loop {
                    let should_continue = engine
                        .process_turn(prompt, &cancel_token, &on_completion_call, 100)
                        .await;

                    if !should_continue || cancel_token.load(Ordering::SeqCst) {
                        break;
                    }

                    // Reset the prompt, as if we are continuing chat
                    prompt = engine.log_handler.get_user_prompt();
                }
            });

            // Loop ended (clean finish, cancel, or error): the chat needs user attention
            let chat_title = title_for_hooks
                .read()
                .ok()
                .and_then(|t| t.clone())
                .unwrap_or_default();
            if let Ok(active) = active_hooks.read() {
                run_needs_attention_hooks(&get_application_config().hooks, &active, &chat_title);
            }
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
    use crate::chat::history::{ChatHistory, load_history_entries};
    use crate::config::TenonConfig;
    use crate::hooks::{Hook, HookType};
    use crate::tools::{TenonTool, use_choreo::UseChoreo};
    use rig::tool::DynamicTool;
    use std::path::Path;
    use std::time::{Duration, Instant};

    fn in_workspace(name: &str, test: impl FnOnce()) {
        const CHILD_ENV: &str = "TENON_SKILL_HISTORY_TEST";
        if std::env::var_os(CHILD_ENV).is_none() {
            let dir =
                std::env::temp_dir().join(format!("tenon-skill-history-{}", uuid::Uuid::new_v4()));
            std::fs::create_dir_all(&dir).unwrap();
            let result = std::process::Command::new(std::env::current_exe().unwrap())
                .arg(format!("chat::tests::{name}"))
                .arg("--exact")
                .env(CHILD_ENV, "1")
                .current_dir(&dir)
                .status();
            std::fs::remove_dir_all(&dir).unwrap();
            assert!(result.unwrap().success());
            return;
        }

        crate::utils::PLUGIN_ROOT
            .set(std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")))
            .ok();
        let mut config = TenonConfig::default();
        let disabled = config.agents[&config.default_agent].clone();
        let mut enabled = disabled.clone();
        enabled.project_skills = true;
        config.agents.insert("enabled".into(), enabled);
        config.agents.insert("disabled".into(), disabled);
        config.default_agent = "enabled".into();
        let _config = crate::ConfigOverride::set(config);
        test();
    }

    fn write_review_skill(description: &str, body: &str) {
        std::fs::create_dir_all(".agents/skills/review").unwrap();
        std::fs::write(
            ".agents/skills/review/SKILL.md",
            format!("---\nname: \"review\"\ndescription: \"{description}\"\n---\n{body}"),
        )
        .unwrap();
    }

    fn load_session_history(id: &str) -> ChatHistory {
        load_history_entries()
            .into_iter()
            .find(|history| history.id == id)
            .unwrap()
    }

    fn saved_review_session() -> (ChatSession, ChatHistory) {
        let mut session = ChatSession::with_agent_name("enabled".into()).unwrap();
        let tool: DynamicTool = TenonTool::new(
            UseChoreo {
                choreos: session.engine.choreos(),
                active_choreo: session.engine.active_choreo.clone(),
            },
            session.engine.log_handler.log_window.clone(),
        )
        .into();
        futures::executor::block_on(tool.run_with(
            &mut session.engine.tool_context,
            serde_json::json!({"choreo_id": "review"}),
        ))
        .unwrap();
        let history = save_session(&session);
        (session, history)
    }

    fn save_session(session: &ChatSession) -> ChatHistory {
        save_to_history(
            SessionMetadata {
                id: &session.id,
                title: None,
                agent_name: &session.active_agent_name,
                model_display: "llama3",
                session_datetime: session.session_datetime,
            },
            &session.engine.log_handler.log_window.read().unwrap(),
            &session.usage,
            &session.work_queue,
        );
        load_session_history(&session.id)
    }

    #[test]
    fn saved_skill_history_keeps_id_without_path_or_instructions() {
        in_workspace(
            "saved_skill_history_keeps_id_without_path_or_instructions",
            || {
                write_review_skill("Review changes", "Private body");

                let (_, history) = saved_review_session();

                let json = serde_json::to_string(&history).unwrap();
                assert!(!json.contains("Private body"));
                assert!(!json.contains("project_skill_base"));
                assert!(matches!(
                    history.logs.last().map(|log| log.data()),
                    Some(TenonLogData::Choreo(log)) if log.id == "review" && log.r#move == Some(1)
                ));
            },
        );
    }

    #[test]
    fn restoring_session_reloads_an_available_skill() {
        in_workspace("restoring_session_reloads_an_available_skill", || {
            write_review_skill("Review changes", "Private body");
            std::fs::rename(".agents/skills/review", ".agents/skills/review-files").unwrap();
            let (enabled, history) = saved_review_session();
            let mut switched = ChatSession::with_agent_name("disabled".into()).unwrap();
            assert!(
                switched
                    .engine
                    .choreos()
                    .iter()
                    .all(|choreo| choreo.id != "review")
            );
            std::fs::write(
                ".agents/skills/review-files/SKILL.md",
                "---\nname: \"review\"\ndescription: \"Review staged changes\"\n---\nNew instructions",
            )
            .unwrap();
            assert_eq!(
                enabled
                    .engine
                    .choreos()
                    .iter()
                    .find(|choreo| choreo.id == "review")
                    .unwrap()
                    .description,
                "Review changes"
            );

            let restored = ChatSession::from_history(history.clone()).unwrap();
            let mut opted_out = ChatSession::from_history(ChatHistory {
                agent_name: "disabled".into(),
                ..history
            })
            .unwrap();
            let (opted_out_messages, _) =
                futures::executor::block_on(opted_out.engine.request_messages("Continue".into()));

            assert!(opted_out.engine.active_choreo.read().unwrap().is_some());
            assert!(opted_out_messages.iter().any(|message| matches!(
                message, rig::message::Message::System { content } if content.contains("New instructions")
            )));
            assert_eq!(restored.id, enabled.id);
            assert_eq!(
                restored
                    .engine
                    .choreos()
                    .iter()
                    .find(|choreo| choreo.id == "review")
                    .map(|choreo| choreo.description),
                Some("Review changes")
            );
            let active = restored.engine.active_choreo.read().unwrap();
            let active = active.as_ref().unwrap();
            assert_eq!(active.r#move, 1);
            assert_eq!(active.choreo.id, "review");
            assert_eq!(
                active.choreo.moves[0].instruction.resolve().unwrap(),
                "---\nname: \"review\"\ndescription: \"Review staged changes\"\n---\nNew instructions"
            );

            let mut agent = get_application_config().agents["enabled"].clone();
            agent.choreos.push(Arc::new(choreo::Choreo {
                id: "review".into(),
                title: "Agent review".into(),
                moves: vec![],
                description: "Agent review description".into(),
            }));
            switched.switch_agent("enabled".into(), &agent);
            assert_eq!(
                switched
                    .engine
                    .choreos()
                    .iter()
                    .map(|choreo| choreo.description)
                    .collect::<Vec<_>>(),
                ["Review changes", "Agent review description"]
            );
            let catalog = UseChoreo {
                choreos: switched.engine.choreos(),
                active_choreo: switched.engine.active_choreo.clone(),
            };
            use crate::tools::ToolCore;
            let description = catalog.description();
            assert!(
                description.find("review: Review changes").unwrap()
                    < description
                        .find("review: Agent review description")
                        .unwrap()
            );
            let start: DynamicTool =
                TenonTool::new(catalog, switched.engine.log_handler.log_window.clone()).into();
            futures::executor::block_on(start.run_with(
                &mut switched.engine.tool_context,
                serde_json::json!({"choreo_id": "review"}),
            ))
            .unwrap();
            assert!(matches!(
                switched
                    .engine
                    .active_choreo
                    .read()
                    .unwrap()
                    .as_ref()
                    .unwrap()
                    .choreo
                    .moves
                    .as_slice(),
                [choreo::Move {
                    instruction: choreo::Instruction::ProjectSkill { base_path },
                    ..
                }] if base_path.as_ref() == Path::new(".agents/skills/review-files")
            ));
        });
    }

    #[test]
    fn skill_discovery_is_cached_globally() {
        in_workspace("skill_discovery_is_cached_globally", || {
            let mut session = ChatSession::with_agent_name("disabled".into()).unwrap();
            write_review_skill("Original description", "Original instructions");
            let enabled = get_application_config().agents["enabled"].clone();
            let disabled = get_application_config().agents["disabled"].clone();
            session.switch_agent("enabled".into(), &enabled);
            let (_, history) = saved_review_session();
            session.engine.load(history.logs.clone());
            write_review_skill("Changed description", "Changed instructions");
            std::fs::create_dir_all(".agents/skills/new").unwrap();
            std::fs::write(
                ".agents/skills/new/SKILL.md",
                "---\nname: new\ndescription: New skill\n---\nInstructions",
            )
            .unwrap();

            let (messages, _) =
                futures::executor::block_on(session.engine.request_messages("Continue".into()));
            assert!(messages.iter().any(|message| matches!(
                message, rig::message::Message::System { content }
                    if content.contains("Changed instructions")
            )));
            session.switch_agent("disabled".into(), &disabled);
            std::fs::rename(".agents/skills/review", ".agents/skills/relocated").unwrap();
            session.engine.load(history.logs);

            assert!(matches!(
                session
                    .engine
                    .active_choreo
                    .read()
                    .unwrap()
                    .as_ref()
                    .unwrap()
                    .choreo
                    .moves
                    .as_slice(),
                [choreo::Move {
                    instruction: choreo::Instruction::ProjectSkill { base_path },
                    ..
                }] if base_path.as_ref() == Path::new(".agents/skills/review")
            ));
            session.switch_agent("enabled".into(), &enabled);
            assert_eq!(
                session
                    .engine
                    .choreos()
                    .iter()
                    .find(|choreo| choreo.id == "review")
                    .unwrap()
                    .description,
                "Original description"
            );
            assert!(
                session
                    .engine
                    .choreos()
                    .iter()
                    .all(|choreo| choreo.id != "new")
            );

            let fresh = ChatSession::with_agent_name("enabled".into()).unwrap();
            assert_eq!(
                fresh
                    .engine
                    .choreos()
                    .iter()
                    .find(|choreo| choreo.id == "review")
                    .unwrap()
                    .description,
                "Original description"
            );
            assert!(
                fresh
                    .engine
                    .choreos()
                    .iter()
                    .all(|choreo| choreo.id != "new")
            );

            let existing_clone = fresh.engine.clone();
            project_skills::discover();

            for engine in [&fresh.engine, &existing_clone] {
                assert_eq!(
                    engine
                        .choreos()
                        .iter()
                        .find(|choreo| choreo.id == "review")
                        .map(|choreo| choreo.description),
                    Some("Changed description")
                );
            }
            let refreshed = ChatSession::with_agent_name("enabled".into()).unwrap();
            assert_eq!(
                refreshed
                    .engine
                    .choreos()
                    .iter()
                    .find(|choreo| choreo.id == "review")
                    .unwrap()
                    .description,
                "Changed description"
            );
            assert!(
                refreshed
                    .engine
                    .choreos()
                    .iter()
                    .any(|choreo| choreo.id == "new")
            );

            let original_dir = std::env::current_dir().unwrap();
            std::fs::create_dir("another-workspace").unwrap();
            std::env::set_current_dir("another-workspace").unwrap();
            let other = ChatSession::with_agent_name("enabled".into()).unwrap();
            assert!(other.engine.choreos().is_empty());
            std::env::set_current_dir(original_dir).unwrap();
        });
    }

    #[test]
    fn unreadable_skill_refreshes_cache_and_retries_by_id() {
        in_workspace("unreadable_skill_refreshes_cache_and_retries_by_id", || {
            for opted_in in [true, false] {
                write_review_skill("Original description", "Original instructions");
                project_skills::discover();
                let (mut session, _) = saved_review_session();
                let enabled = get_application_config().agents["enabled"].clone();
                if !opted_in {
                    let disabled = get_application_config().agents["disabled"].clone();
                    session.switch_agent("disabled".into(), &disabled);
                }
                session
                    .engine
                    .active_choreo
                    .write()
                    .unwrap()
                    .as_mut()
                    .unwrap()
                    .memory
                    .insert("prior".into(), "preserved".into());
                std::fs::rename(".agents/skills/review", ".agents/skills/relocated").unwrap();
                std::fs::write(
                    ".agents/skills/relocated/SKILL.md",
                    "---\nname: review\ndescription: Refreshed description\n---\nRelocated instructions",
                )
                .unwrap();
                std::fs::create_dir_all(".agents/skills/new").unwrap();
                std::fs::write(
                    ".agents/skills/new/SKILL.md",
                    "---\nname: new\ndescription: New skill\n---\nInstructions",
                )
                .unwrap();

                let mut request_engine = session.engine.clone();
                let (messages, _) =
                    futures::executor::block_on(request_engine.request_messages("Continue".into()));

                assert!(messages.iter().any(|message| matches!(
                    message, rig::message::Message::System { content }
                        if content.contains("Relocated instructions")
                )));
                {
                    let active = session.engine.active_choreo.read().unwrap();
                    let active = active.as_ref().unwrap();
                    assert_eq!(active.choreo.id, "review");
                    assert!(matches!(
                        active.choreo.moves.as_slice(),
                        [choreo::Move {
                            instruction: choreo::Instruction::ProjectSkill { base_path },
                            ..
                        }] if base_path.as_ref() == Path::new(".agents/skills/relocated")
                    ));
                    assert_eq!(
                        active.memory.get("prior").map(String::as_str),
                        Some("preserved")
                    );
                }
                assert_eq!(
                    request_engine
                        .choreos()
                        .iter()
                        .any(|choreo| choreo.id == "new"),
                    opted_in
                );
                if opted_in {
                    assert_eq!(
                        request_engine
                            .choreos()
                            .iter()
                            .find(|choreo| choreo.id == "review")
                            .unwrap()
                            .description,
                        "Refreshed description"
                    );
                }
                session.switch_agent("enabled".into(), &enabled);
                assert_eq!(
                    session
                        .engine
                        .choreos()
                        .iter()
                        .find(|choreo| choreo.id == "review")
                        .unwrap()
                        .description,
                    "Refreshed description"
                );
                assert!(
                    session
                        .engine
                        .choreos()
                        .iter()
                        .any(|choreo| choreo.id == "new")
                );
                let other = ChatSession::with_agent_name("enabled".into()).unwrap();
                assert_eq!(
                    other
                        .engine
                        .choreos()
                        .iter()
                        .find(|choreo| choreo.id == "review")
                        .unwrap()
                        .description,
                    "Refreshed description"
                );

                std::fs::remove_dir_all(".agents/skills/relocated").unwrap();
                let (messages, _) =
                    futures::executor::block_on(request_engine.request_messages("Continue".into()));
                assert!(request_engine.active_choreo.read().unwrap().is_none());
                let window = request_engine.log_handler.log_window.read().unwrap();
                let log = window.logs.last().unwrap().log.read().unwrap();
                let TenonLogData::Choreo(end) = log.data() else {
                    panic!("expected a choreo end event");
                };
                let artifact = end.tool_log.tool_call.args["move_artifact"]
                    .as_str()
                    .unwrap();
                assert!(artifact.to_lowercase().contains("instruction"));
                assert!(messages.iter().any(|message| matches!(
                    message, rig::message::Message::System { content } if content.contains(artifact)
                )));
                assert!(
                    request_engine
                        .choreos()
                        .iter()
                        .all(|choreo| choreo.id != "review")
                );
                assert_eq!(
                    request_engine
                        .choreos()
                        .iter()
                        .any(|choreo| choreo.id == "new"),
                    opted_in
                );
                std::fs::remove_dir_all(".agents/skills").unwrap();
            }
        });
    }

    #[test]
    fn skill_instruction_read_error_refreshes_discovery() {
        in_workspace("skill_instruction_read_error_refreshes_discovery", || {
            write_review_skill("Review changes", "Valid instructions");
            let (mut session, _) = saved_review_session();
            std::fs::create_dir_all("unreadable").unwrap();
            std::fs::write("unreadable/SKILL.md", [0xff]).unwrap();
            {
                let mut active = session.engine.active_choreo.write().unwrap();
                let active = active.as_mut().unwrap();
                let mut choreo = (*active.choreo).clone();
                choreo.moves[0].instruction = choreo::Instruction::ProjectSkill {
                    base_path: crate::utils::path::TenonPath::with_cwd("unreadable"),
                };
                active.choreo = Arc::new(choreo);
            }

            let (messages, _) =
                futures::executor::block_on(session.engine.request_messages("Continue".into()));

            assert!(session.engine.active_choreo.read().unwrap().is_some());
            assert!(messages.iter().any(|message| matches!(
                message, rig::message::Message::System { content }
                    if content.contains("Valid instructions")
            )));
        });
    }

    #[test]
    fn native_instruction_failure_refreshes_skill_discovery() {
        in_workspace(
            "native_instruction_failure_refreshes_skill_discovery",
            || {
                write_review_skill("Original description", "Original instructions");
                let mut session = ChatSession::with_agent_name("enabled".into()).unwrap();
                let native = Arc::new(choreo::Choreo {
                    id: "native".into(),
                    title: "Native".into(),
                    description: String::new(),
                    moves: vec![choreo::Move {
                        title: "Run".into(),
                        instruction: choreo::Instruction::File {
                            file: "missing-instruction".into(),
                        },
                        goto_instructions: vec![],
                    }],
                });
                *session.engine.active_choreo.write().unwrap() = Some(ActiveChoreo::new(native, 1));
                write_review_skill("Changed description", "Changed instructions");

                futures::executor::block_on(session.engine.request_messages("Continue".into()));
                let enabled = get_application_config().agents["enabled"].clone();
                session.switch_agent("enabled".into(), &enabled);

                assert!(session.engine.active_choreo.read().unwrap().is_none());
                assert_eq!(
                    session
                        .engine
                        .choreos()
                        .iter()
                        .find(|choreo| choreo.id == "review")
                        .unwrap()
                        .description,
                    "Changed description"
                );
            },
        );
    }

    #[test]
    fn restoring_missing_skill_defers_saving_its_end() {
        in_workspace("restoring_missing_skill_defers_saving_its_end", || {
            write_review_skill("Review changes", "Private body");
            let (enabled, history) = saved_review_session();
            std::fs::remove_dir_all(".agents/skills/review").unwrap();

            let mut recovered = ChatSession::from_history(history.clone()).unwrap();
            write_review_skill("Review changes", "Restored before prompt");
            let (messages, _) =
                futures::executor::block_on(recovered.engine.request_messages("Continue".into()));
            assert!(recovered.engine.active_choreo.read().unwrap().is_some());
            assert!(messages.iter().any(|message| matches!(
                message, rig::message::Message::System { content }
                    if content.contains("Restored before prompt")
            )));
            std::fs::remove_dir_all(".agents/skills/review").unwrap();

            let mut restored = ChatSession::from_history(ChatHistory {
                agent_name: "disabled".into(),
                ..history.clone()
            })
            .unwrap();
            futures::executor::block_on(restored.engine.request_messages("Continue".into()));

            assert!(restored.engine.active_choreo.read().unwrap().is_none());
            {
                let window = restored.engine.log_handler.log_window.read().unwrap();
                assert_eq!(window.logs.len(), history.logs.len() + 1);
                let log = window.logs.last().unwrap().log.read().unwrap();
                let TenonLogData::Choreo(end) = log.data() else {
                    panic!("expected a choreo end event in memory");
                };
                assert_eq!(end.id, "review");
                assert_eq!(end.r#move, None);
                assert_eq!(end.content, "Choreo ended");
                let artifact = end.tool_log.tool_call.args["move_artifact"]
                    .as_str()
                    .unwrap();
                assert!(artifact.to_lowercase().contains("instruction"));
                assert!(end.system_content().contains(artifact));
            }
            let unchanged = load_session_history(&enabled.id);
            assert_eq!(
                serde_json::to_value(&unchanged).unwrap(),
                serde_json::to_value(&history).unwrap()
            );

            write_review_skill("Review changes", "Back on disk");
            let reopened = ChatSession::from_history(unchanged).unwrap();
            assert_eq!(
                reopened
                    .engine
                    .active_choreo
                    .read()
                    .unwrap()
                    .as_ref()
                    .unwrap()
                    .choreo
                    .id,
                "review"
            );

            let ended = save_session(&restored);
            assert_eq!(ended.logs.len(), history.logs.len() + 1);
            let reopened = ChatSession::from_history(ended.clone()).unwrap();
            assert!(reopened.engine.active_choreo.read().unwrap().is_none());
            assert_eq!(
                reopened
                    .engine
                    .log_handler
                    .log_window
                    .read()
                    .unwrap()
                    .logs
                    .len(),
                ended.logs.len()
            );
        });
    }

    /// Waits until the marker file exists and returns its content.
    fn wait_for_marker(path: &std::path::Path) -> String {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if let Ok(content) = std::fs::read_to_string(path) {
                return content;
            }
            assert!(Instant::now() < deadline, "hook did not run in time");
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    #[test]
    fn active_skill_rereads_instructions_and_ends_when_deleted() {
        in_workspace(
            "active_skill_rereads_instructions_and_ends_when_deleted",
            || {
                write_review_skill("Review changes", "First instructions");
                let (mut session, _) = saved_review_session();
                let agent = get_application_config().agents["disabled"].clone();
                session.switch_agent("disabled".into(), &agent);

                let (messages, prompt) = futures::executor::block_on(
                    session.engine.request_messages("Review this".into()),
                );
                assert!(
                    session
                        .engine
                        .choreos()
                        .iter()
                        .all(|choreo| choreo.id != "review")
                );
                assert_eq!(prompt, rig::message::Message::user("Review this"));
                assert!(messages.iter().any(|message| matches!(
                    message, rig::message::Message::System { content }
                        if content.contains("First instructions")
                            && content.contains("Skill base directory: ./.agents/skills/review")
                            && content.contains("Follow the entire SKILL.md")
                            && content.contains("call `end_choreo` without a `move_artifact`")
                )));

                write_review_skill("Review changes", "Updated instructions");
                let (messages, prompt) =
                    futures::executor::block_on(session.engine.request_messages(String::new()));
                assert_eq!(prompt, rig::message::Message::user("<context></context>"));
                assert!(messages.iter().any(|message| matches!(
                message, rig::message::Message::System { content }
                    if content.contains("Updated instructions") && !content.contains("First instructions")
            )));

                std::fs::remove_dir_all(".agents/skills/review").unwrap();
                let (messages, prompt) =
                    futures::executor::block_on(session.engine.request_messages(String::new()));
                assert_eq!(prompt, rig::message::Message::user("<context></context>"));
                assert!(session.engine.active_choreo.read().unwrap().is_none());
                let window = session.engine.log_handler.log_window.read().unwrap();
                let log = window.logs.last().unwrap().log.read().unwrap();
                let TenonLogData::Choreo(ended) = log.data() else {
                    panic!("expected an end-choreo log");
                };
                assert_eq!(ended.id, "review");
                assert_eq!(ended.r#move, None);
                let artifact = ended.tool_log.tool_call.args["move_artifact"]
                    .as_str()
                    .unwrap();
                assert!(artifact.to_lowercase().contains("instruction"));
                assert!(messages.iter().any(|message| matches!(
                    message, rig::message::Message::System { content } if content.contains(artifact)
                )));
            },
        );
    }

    #[test]
    fn switching_agent_replaces_model_directives_tools_and_choreos() {
        let config = TenonConfig::default();
        let mut configured = config.agents[&config.default_agent].clone();
        let _config = crate::ConfigOverride::set(config);
        let mut session = ChatSession::new();
        configured.model.id = "switched-model".into();
        configured.directive = vec![crate::directive::Directive {
            condition: Some("when testing".into()),
            source: crate::directive::DirectiveSource::Preset {
                id: "test-directive".into(),
                content: crate::directive::PresetContent::Text("follow this directive".into()),
            },
        }];
        configured.tool_names = vec!["read_file".into(), "edit_file".into()];
        configured.choreos = vec![Arc::new(choreo::Choreo {
            id: "test-choreo".into(),
            title: "Test choreo".into(),
            moves: vec![],
            description: "Test choreo description".into(),
        })];

        session.switch_agent("configured".into(), &configured);

        assert_eq!(session.active_agent_name, "configured");
        assert_eq!(session.engine.model.id, "switched-model");
        assert_eq!(
            session.engine.directive[0].resolve().unwrap(),
            "<directive condition=\"when testing\" preset=\"test-directive\">follow this directive</directive>"
        );
        assert_eq!(
            session
                .engine
                .tools
                .iter()
                .map(|tool| tool.name())
                .collect::<Vec<_>>(),
            vec!["read_file", "edit_file"]
        );
        assert_eq!(
            session
                .engine
                .choreos()
                .iter()
                .map(|c| c.id)
                .collect::<Vec<_>>(),
            ["test-choreo"]
        );

        configured.model.id = "another-model".into();
        configured.directive.clear();
        configured.tool_names = vec!["search_text".into()];
        configured.choreos.clear();
        session.engine.switch_agent(&configured);

        assert_eq!(session.active_agent_name, "configured");
        assert_eq!(session.engine.model.id, "another-model");
        assert!(session.engine.directive.is_empty());
        assert_eq!(
            session
                .engine
                .tools
                .iter()
                .map(|tool| tool.name())
                .collect::<Vec<_>>(),
            vec!["search_text"]
        );
        assert!(session.engine.choreos().is_empty());
    }

    #[test]
    fn push_pending_action_fires_needs_attention_hook_once() {
        crate::utils::PLUGIN_ROOT
            .set(std::env::current_dir().unwrap())
            .ok();

        let dir = std::env::temp_dir().join(format!("tenon_pending_hook_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let marker = dir.join("fired");

        // The hook appends a line per run so repeated fires are observable.
        let config = TenonConfig {
            hooks: vec![Hook {
                name: "attention".to_string(),
                command: format!("echo fired >> {}", marker.to_str().unwrap()),
                enabled: true,
                hook_type: HookType::NeedsAttention,
            }],
            ..TenonConfig::default()
        };
        let _override = crate::ConfigOverride::set(config);

        let session = ChatSession::new();
        session
            .pending_actions_channel
            .push(PendingAction::Question(Question {
                question: "question needing attention".to_string(),
                open_ended_text: None,
                options: vec![],
                response_tx: Arc::new(Mutex::new(None)),
            }));

        let content = wait_for_marker(&marker);
        assert_eq!(content.lines().count(), 1);

        // Re-subscribing (what a session switch does) must not re-fire the hook.
        let _rx = session.pending_actions_channel.subscribe();
        std::thread::sleep(Duration::from_millis(100));
        let content = std::fs::read_to_string(&marker).unwrap();
        assert_eq!(content.lines().count(), 1);

        let _ = std::fs::remove_dir_all(&dir);
    }

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
