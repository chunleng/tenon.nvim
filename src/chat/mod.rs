use crate::{
    chat::workflow::GotoStep,
    clients::{Behavior, BehaviorSource, ChatAgent, StreamItem, SupportedModels, get_agent},
    config::user::WorkflowConfig,
    get_application_config, get_workflow_registry,
    tools::resolve_tools,
    utils::GLOBAL_EXECUTION_HANDLER,
};
use chrono::{DateTime, Local};
use fastembed::{EmbeddingModel, InitOptions, TextEmbedding};
use nvim_oxi::{Result as OxiResult, api::types::LogLevel};
use rig::{
    OneOrMany,
    completion::Usage,
    message::{Message, ToolResultContent, UserContent},
};
use std::{
    sync::atomic::{AtomicBool, AtomicUsize, Ordering},
    sync::{Arc, LazyLock, Mutex, RwLock},
};

const MAX_CONTEXT_TOKENS: usize = 20_000;

pub mod history;
pub mod log;
pub mod workflow;

pub use log::{
    TenonAssistantMessage, TenonAssistantMessageContent, TenonLog, TenonLogData, TenonToolCall,
    TenonToolError, TenonToolLog, TenonToolResult, TenonUserMessage, TenonUserTextMessage,
    TenonWorkflowLog,
};

use history::save_to_history;

/// Builds a workflow-wrapped prompt if there's an active workflow.
fn build_workflow_prompt(
    active_workflow: &Arc<RwLock<Option<ActiveWorkflow>>>,
    base_prompt: String,
) -> String {
    if let Ok(active_lock) = active_workflow.read() {
        if let Some(ref active) = active_lock.as_ref() {
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
                        let target_step = match &instr.to {
                            GotoStep::Next => active.step + 1,
                            GotoStep::Step(n) => *n,
                        };
                        if target_step > total_steps {
                            format!(
                                "{}end_workflow output:{}",
                                instr
                                    .condition
                                    .as_ref()
                                    .map(|x| { format!("{} → ", x) })
                                    .unwrap_or("".to_string()),
                                instr.output
                            )
                        } else {
                            format!(
                                "{}navigate_output step:{} output:{}",
                                instr
                                    .condition
                                    .as_ref()
                                    .map(|x| { format!("{} → ", x) })
                                    .unwrap_or("".to_string()),
                                target_step,
                                instr.output
                            )
                        }
                    })
                    .collect();

                // Only add default ending if at last step and no goto already ends workflow
                if active.step == total_steps {
                    let has_ending_goto = step.goto_instructions.iter().any(|instr| {
                        let target_step = match &instr.to {
                            GotoStep::Next => active.step + 1,
                            GotoStep::Step(n) => *n,
                        };
                        target_step > total_steps
                    });
                    if !has_ending_goto {
                        goto_lines.push("end_workflow output:nothing".to_string());
                    }
                }

                let goto_instruction = goto_lines.join("\n");

                return format!(
                    "<context>\n\
                    Currently in {} step of {} workflow. In a workflow, user prompt first, workflow instruction second and chat history is just reference\n\
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

/// Converts a TenonLog to a string for embedding.
/// Extracts the text content from the log for semantic search.
fn log_to_text(log: &TenonLog) -> Option<String> {
    match log.data() {
        TenonLogData::User(msg) => match msg {
            TenonUserMessage::Text(TenonUserTextMessage(text)) => Some(text.clone()),
        },
        TenonLogData::Assistant(msg) => Some(
            msg.content
                .iter()
                .filter_map(|c| match c {
                    TenonAssistantMessageContent::Text(t) => Some(t.clone()),
                })
                .collect::<Vec<_>>()
                .join("\n"),
        ),
        TenonLogData::Tool(tool_log) => {
            let mut text = format!(
                "Tool: {}\nArgs: {}",
                tool_log.tool_call.name, tool_log.tool_call.args
            );
            if let Some(result) = &tool_log.tool_result {
                match result {
                    Ok(TenonToolResult::Text(t)) => {
                        text.push_str(&format!("\nResult: {}", t.text));
                    }
                    Ok(TenonToolResult::Image(_)) => {
                        text.push_str("\nResult: [Image]");
                    }
                    Err(e) => {
                        text.push_str(&format!("\nError: {}", e.0));
                    }
                }
            }
            Some(text)
        }
        TenonLogData::Workflow(_) => None, // Workflow does not need to be indexed for RAG
    }
}

/// Generates embeddings for a list of texts using FastEmbed.
/// Returns a vector of embedding vectors.
fn generate_embeddings(texts: &[String]) -> Option<Vec<Vec<f64>>> {
    if texts.is_empty() {
        return None;
    }

    // Use ~/.fastembed_cache for model storage
    let cache_dir = std::env::var("HOME")
        .map(|home| std::path::PathBuf::from(home).join(".fastembed_cache"))
        .unwrap_or_else(|_| std::path::PathBuf::from(".fastembed_cache"));

    let options = InitOptions::new(EmbeddingModel::BGESmallENV15)
        .with_cache_dir(cache_dir)
        .with_show_download_progress(false);

    let model = TextEmbedding::try_new(options).ok()?;

    // Generate embeddings (batch_size = None for default)
    let embeddings = model
        .embed(texts.iter().map(|s| s.as_str()).collect::<Vec<_>>(), None)
        .ok()?;

    // Convert Vec<Vec<f32>> to Vec<Vec<f64>>
    Some(
        embeddings
            .into_iter()
            .map(|v| v.into_iter().map(|f| f as f64).collect())
            .collect(),
    )
}

/// Computes cosine similarity between two vectors.
fn cosine_similarity(a: &[f64], b: &[f64]) -> f64 {
    let dot: f64 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
    let norm_a: f64 = a.iter().map(|x| x * x).sum::<f64>().sqrt();
    let norm_b: f64 = b.iter().map(|x| x * x).sum::<f64>().sqrt();
    if norm_a == 0.0 || norm_b == 0.0 {
        0.0
    } else {
        dot / (norm_a * norm_b)
    }
}

/// Finds the top-k most similar logs to the query embedding.
/// Returns indices into the rag_logs array.
fn find_top_k_similar(query_embedding: &[f64], embeddings: &[Vec<f64>], k: usize) -> Vec<usize> {
    let mut similarities: Vec<(usize, f64)> = embeddings
        .iter()
        .enumerate()
        .map(|(i, emb)| (i, cosine_similarity(query_embedding, emb)))
        .collect();

    similarities.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

    similarities.into_iter().take(k).map(|(i, _)| i).collect()
}

/// Gets cached embeddings or generates new ones for the given logs.
/// Incrementally generates embeddings only for logs that don't have them cached.
fn get_or_generate_embeddings(
    logs: &[TenonLog],
    cache: &Arc<RwLock<Option<Vec<Vec<f64>>>>>,
) -> Option<Vec<Vec<f64>>> {
    let cached_len = cache.read().ok()?.as_ref().map(|c| c.len()).unwrap_or(0);

    // Check if cache has all embeddings
    if cached_len == logs.len() {
        return cache.read().ok()?.as_ref().cloned();
    }

    // Cache is missing some embeddings - generate only for new logs
    if cached_len < logs.len() {
        // Generate embeddings for logs that don't have them yet
        let new_texts: Vec<_> = logs[cached_len..].iter().filter_map(log_to_text).collect();
        let new_embeddings = generate_embeddings(&new_texts)?;

        // Append to existing cache
        if let Ok(mut lock) = cache.write() {
            match lock.as_mut() {
                Some(existing) => existing.extend(new_embeddings),
                None => *lock = Some(new_embeddings),
            }
        }

        return cache.read().ok()?.as_ref().cloned();
    }

    // Cache has more embeddings than logs (shouldn't happen, but regenerate to be safe)
    let texts: Vec<_> = logs.iter().filter_map(log_to_text).collect();
    let embeddings = generate_embeddings(&texts)?;

    if let Ok(mut lock) = cache.write() {
        *lock = Some(embeddings.clone());
    }

    Some(embeddings)
}

/// Builds RAG context string by finding similar past conversation logs.
/// Returns None if no relevant context is found.
fn build_rag_context(
    rag_logs: &Arc<RwLock<Vec<TenonLog>>>,
    rag_embeddings: &Arc<RwLock<Option<Vec<Vec<f64>>>>>,
    message: &str,
) -> Option<String> {
    let logs = rag_logs.read().ok()?;
    if logs.is_empty() {
        return None;
    }

    let embeddings = get_or_generate_embeddings(&logs, rag_embeddings)?;
    let msg_embedding = generate_embeddings(&[message.to_string()])?
        .into_iter()
        .next()?;

    let top_indices = find_top_k_similar(&msg_embedding, &embeddings, 3);
    let context_parts: Vec<_> = top_indices
        .into_iter()
        .filter_map(|i| logs.get(i))
        .filter_map(log_to_text)
        .collect();

    (!context_parts.is_empty()).then(|| {
        format!(
            "Relevant context from earlier conversation:\n{}\n\n",
            context_parts.join("\n---\n")
        )
    })
}

pub struct ChatSession {
    pub id: String,
    pub title: Arc<RwLock<Option<String>>>,
    pub logs: Arc<RwLock<Vec<TenonLog>>>,
    pub rag_logs: Arc<RwLock<Vec<TenonLog>>>,
    pub rag_embeddings: Arc<RwLock<Option<Vec<Vec<f64>>>>>,
    pub resume_from: Arc<AtomicUsize>,
    pub usage: Arc<RwLock<Option<Usage>>>,
    pub active_agent: ActiveAgent,
    pub active_workflow: Arc<RwLock<Option<ActiveWorkflow>>>,
    pub session_datetime: DateTime<Local>,
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
    pub behavior: Vec<Behavior>,
    pub tool_names: Vec<String>,
    pub workflows: Vec<WorkflowConfig>,
}

impl TenonAgent {
    pub fn new(
        model: SupportedModels,
        behavior: Vec<Behavior>,
        tools: &[impl AsRef<str>],
        workflows: Vec<WorkflowConfig>,
    ) -> Self {
        Self {
            model,
            behavior,
            tool_names: tools.iter().map(|t| t.as_ref().to_string()).collect(),
            workflows,
        }
    }

    pub fn build_chat_adapter(
        &self,
        session_datetime: DateTime<Local>,
        workflow_context: Arc<RwLock<Option<ActiveWorkflow>>>,
        logs: Arc<RwLock<Vec<TenonLog>>>,
    ) -> ChatAgent {
        // NOTE: Update token estimation when this prompt changes
        let mut system_prompt = "Output markdown. Concise, not verbose. No filler or hedging or unnecessary words. Reduce emoji use. \
            User may edit files between steps → files change silently. File ≠ expected → user edited → re-read → preserve changes. \
            History shows active behavior/prompt at that time. Prior actions may span agents → trust reported behavior. \
            Earlier history may be truncated. Missing context → ask user for clarification. \
            <knowledge></knowledge>=important info agent knows. <behavior></behavior>=rules for agent conduct; no condition→always, condition→when matched. Both take priority; only explicit user instruction overrides them.".to_string();

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
                    Some(format!("{} -> {}", w.id, condition))
                })
                .collect();

            system_prompt.push_str(&format!(
                " You have workflow to help you solve problems, always prioritize workflow, if they match the condition, over manually figuring out a process\n\
                In workflow + question for user → ask directly. Never via navigate/end_workflow.\n<workflows>{}</workflows>",
                workflow_info.join(", ")
            ));
        }

        system_prompt = format!(
            "{} Session started: {}",
            system_prompt,
            session_datetime.format("%a %b %d, %Y %H:%M %Z").to_string()
        );

        let mut combined = vec![Behavior {
            condition: None,
            source: BehaviorSource::Text {
                value: system_prompt,
            },
        }];
        combined.extend(self.behavior.iter().cloned());

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
                    logs: logs.clone(),
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
                logs: logs.clone(),
            }));
            tools.push(Box::new(EndWorkflow {
                active_workflow: workflow_context,
                logs,
            }));
        }

        get_agent(self.model.clone(), combined, tools)
    }

    pub fn token_count(&self) -> usize {
        // TODO: make tool estimate count with actual definition
        // NOTE: Update this when system prompt changes. Last estimate: ~150 tokens.
        self.tool_names.len() * 150 + 150
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
            logs: Arc::new(RwLock::new(Vec::new())),
            rag_logs: Arc::new(RwLock::new(Vec::new())),
            rag_embeddings: Arc::new(RwLock::new(None)),
            resume_from: Arc::new(AtomicUsize::new(0)),
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

        let session = Self {
            id: history.id,
            title: Arc::new(RwLock::new(history.title)),
            logs: Arc::new(RwLock::new(logs)),
            rag_logs: Arc::new(RwLock::new(Vec::new())),
            rag_embeddings: Arc::new(RwLock::new(None)),
            resume_from: Arc::new(AtomicUsize::new(0)),
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

        // Apply truncation on restore
        session.apply_context_truncation();

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

                let behavior = vec![Behavior {
                    condition: None,
                    source: BehaviorSource::Text {
                        value: config.title.prompt.clone(),
                    },
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

    /// Returns true if the log entry is a user message.
    fn is_user_log(log: &TenonLog) -> bool {
        matches!(log.data(), TenonLogData::User(_))
    }

    /// Finds the next user message index starting from `start_idx`.
    /// Returns None if no user message is found.
    fn find_next_user_index(logs: &[TenonLog], start_idx: usize) -> Option<usize> {
        logs.iter()
            .enumerate()
            .skip(start_idx)
            .find(|(_, log)| Self::is_user_log(log))
            .map(|(i, _)| i)
    }

    /// Applies context truncation if token count exceeds MAX_CONTEXT_TOKENS.
    /// Copies truncated logs to rag_logs and updates resume_from.
    /// Logs remain in self.logs for display purposes.
    /// The last user/assistant exchange is always preserved.
    fn apply_context_truncation(&self) {
        if let Ok(logs) = self.logs.read() {
            let current_resume = self.resume_from.load(Ordering::SeqCst);
            let agent_tokens = self.active_agent.token_count();

            // Find the last user message - this is the boundary we cannot cross
            let last_user_idx = logs
                .iter()
                .rposition(|log| Self::is_user_log(log))
                .unwrap_or(0);

            // Minimum boundary: we must keep the last exchange
            let min_resume = last_user_idx.min(logs.len().saturating_sub(1));

            // Calculate tokens from current resume_from
            let mut total_tokens: usize = agent_tokens;
            for log in logs.iter().skip(current_resume) {
                total_tokens += log.token_count();
            }

            // If under threshold, no truncation needed
            if total_tokens <= MAX_CONTEXT_TOKENS {
                return;
            }

            // Need to truncate - find new resume_from
            let mut new_resume = current_resume;

            // Move resume_from forward until we're under threshold
            for log in logs.iter().skip(current_resume) {
                let log_tokens = log.token_count();
                total_tokens -= log_tokens;
                new_resume += 1;

                if total_tokens <= MAX_CONTEXT_TOKENS {
                    break;
                }
            }

            // Adjust to next user message if we landed on non-user
            if new_resume < logs.len() && !Self::is_user_log(&logs[new_resume]) {
                if let Some(user_idx) = Self::find_next_user_index(&logs, new_resume) {
                    new_resume = user_idx;
                }
            }

            // Never truncate past the last exchange
            new_resume = new_resume.min(min_resume);

            // Only update if we're actually moving forward
            if new_resume <= current_resume {
                return;
            }

            // Collect new logs for rag_logs
            let new_logs: Vec<_> = logs
                .iter()
                .skip(current_resume)
                .take(new_resume - current_resume)
                .cloned()
                .collect();

            // Copy truncated logs to rag_logs (keep in self.logs for display)
            if let Ok(mut rag_logs) = self.rag_logs.write() {
                for log in new_logs {
                    rag_logs.push(log);
                }
            }

            // Note: Embeddings cache is NOT invalidated - get_or_generate_embeddings
            // will incrementally generate embeddings for new logs off-thread

            self.resume_from.store(new_resume, Ordering::SeqCst);
        }
    }

    /// Returns the total token count from logs starting at resume_from.
    pub fn total_token_count(&self) -> usize {
        let resume_idx = self.resume_from.load(Ordering::SeqCst);
        if let Ok(logs) = self.logs.read() {
            logs.iter()
                .skip(resume_idx)
                .map(|log| log.token_count())
                .sum::<usize>()
                + self.active_agent.token_count()
        } else {
            0
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
        self.apply_context_truncation();

        let logs_clone = Arc::clone(&self.logs);
        let usage_clone = Arc::clone(&self.usage);
        let agent_clone = self.active_agent.clone();
        let chat_id = self.id.clone();
        let title_clone = Arc::clone(&self.title);
        let session_datetime = self.session_datetime.clone();
        let resume_from = Arc::clone(&self.resume_from);
        let cancel_token = Arc::clone(&self.cancel_token);
        let rag_logs_clone = Arc::clone(&self.rag_logs);
        let rag_embeddings_clone = Arc::clone(&self.rag_embeddings);
        let active_workflow_clone = Arc::clone(&self.active_workflow);

        self.active_thread = Some(std::thread::spawn(move || {
            let rt = tokio::runtime::Runtime::new().unwrap();
            rt.block_on(async {
                let mut prompt = build_workflow_prompt(&active_workflow_clone, prompt);
                // Clean up trailing tool calls without results
                if let Ok(mut logs) = logs_clone.write() {
                    let mut logs_vec: Vec<_> = logs.iter().cloned().collect();
                    logs.clear();

                    // Find where trailing tools start (last non-tool or tool with result)
                    let trailing_start = logs_vec
                        .iter()
                        .rposition(|log| !matches!(log.data(), TenonLogData::Tool(_)))
                        .map(|i| i + 1)
                        .unwrap_or(0);

                    // Keep only tools with results in the trailing section
                    let trailing_tools: Vec<_> = logs_vec[trailing_start..]
                        .iter()
                        .cloned()
                        .filter(|log| {
                            if let TenonLogData::Tool(tool_log) = log.data() {
                                tool_log.tool_result.is_some()
                            } else {
                                true
                            }
                        })
                        .collect();

                    logs_vec.truncate(trailing_start);
                    logs_vec.extend(trailing_tools);

                    for log in logs_vec {
                        logs.push(log);
                    }
                }

                // Build chat_history
                let resume_idx = resume_from.load(Ordering::SeqCst);
                let mut chat_history: Vec<Message> = if let Ok(logs) = logs_clone.read() {
                    logs.iter()
                        .skip(resume_idx)
                        .cloned()
                        .flat_map(|x| Vec::<Message>::from(x))
                        .collect()
                } else {
                    vec![]
                };

                // Add user message if provided
                if let Some(ref msg) = user_message {
                    if let Ok(mut logs) = logs_clone.write() {
                        logs.push(TenonLog::new(TenonLogData::User(TenonUserMessage::Text(
                            TenonUserTextMessage(msg.clone()),
                        ))));
                    }
                }

                // Build RAG context (inside thread to avoid blocking main)
                let rag_context = user_message
                    .as_ref()
                    .and_then(|msg| build_rag_context(&rag_logs_clone, &rag_embeddings_clone, msg));

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
                        session_datetime,
                        active_workflow_clone.clone(),
                        logs_clone.clone(),
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
                                if let Ok(mut logs) = logs_clone.write() {
                                    if let Some(log) = logs.iter_mut().find_map(|x| {
                                        if let TenonLogData::Tool(tool) = x.data() {
                                            if tool.tool_call.internal_call_id == internal_call_id {
                                                return Some(x);
                                            }
                                        }
                                        None
                                    }) {
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
                                                        && let Some(serde_json::Value::String(
                                                            output,
                                                        )) = args_obj.get("output")
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
                            }
                            Ok(StreamItem::ReasoningDelta { reasoning }) => {
                                if let Ok(mut logs) = logs_clone.write() {
                                    let mut updated = false;
                                    if let Some(log) = logs.last_mut() {
                                        updated = log.append_reasoning(&reasoning);
                                    }

                                    if !updated {
                                        logs.push(TenonLog::new(TenonLogData::Assistant(
                                            TenonAssistantMessage {
                                                reasoning: Some(reasoning),
                                                content: vec![],
                                            },
                                        )));
                                    }
                                }
                            }
                            Ok(StreamItem::Text { text }) => {
                                if let Ok(mut logs) = logs_clone.write() {
                                    let mut updated = false;
                                    if let Some(log) = logs.last_mut() {
                                        updated = log.append_text(&text);
                                    }

                                    if !updated {
                                        logs.push(TenonLog::new(TenonLogData::Assistant(
                                            TenonAssistantMessage {
                                                reasoning: None,
                                                content: vec![TenonAssistantMessageContent::Text(
                                                    text,
                                                )],
                                            },
                                        )));
                                    }
                                }
                            }
                            Ok(StreamItem::ToolCall {
                                tool_call,
                                internal_call_id,
                            }) => {
                                if let Ok(mut logs) = logs_clone.write() {
                                    logs.push(TenonLog::new(TenonLogData::Tool(TenonToolLog {
                                        tool_call: TenonToolCall {
                                            id: tool_call.id,
                                            internal_call_id: internal_call_id,
                                            name: tool_call.function.name,
                                            args: tool_call.function.arguments,
                                        },
                                        tool_result: None,
                                    })));
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
                                    session_datetime,
                                    &logs_clone,
                                    &usage_clone,
                                    &history_dir,
                                );
                            }
                            Ok(StreamItem::Other) => {}
                            Err(e) => {
                                let _ = GLOBAL_EXECUTION_HANDLER.notify_on_main_thread(
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

                    let resume_idx = resume_from.load(Ordering::SeqCst);
                    chat_history = if let Ok(logs) = logs_clone.read() {
                        logs.iter()
                            .skip(resume_idx)
                            .cloned()
                            .flat_map(|x| Vec::<Message>::from(x))
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

    // pub fn start_workflow(&mut self, workflow_id: String) -> anyhow::Result<()> {
    //     // Check if agent has this workflow configured
    //     if !self
    //         .active_agent
    //         .inner
    //         .workflows
    //         .iter()
    //         .any(|w| w.id == workflow_id)
    //     {
    //         return Err(anyhow::anyhow!(
    //             "Agent '{}' does not have workflow '{}' configured.",
    //             self.active_agent.name,
    //             workflow_id,
    //         ));
    //     }
    //
    //     // Validate workflow exists in registry
    //     let registry = get_workflow_registry();
    //     let workflow = registry
    //         .get(&workflow_id)
    //         .ok_or_else(|| anyhow::anyhow!("Workflow '{}' not found in registry.", workflow_id,))?;
    //
    //     let step_number = 1;
    //     if let Ok(mut logs) = self.logs.write() {
    //         logs.push(TenonLog::new(TenonLogData::Workflow(
    //             workflow.generate_log(step_number).unwrap(),
    //         )));
    //     }
    //     if let Ok(mut active) = self.active_workflow.write() {
    //         *active = Some(ActiveWorkflow::new(workflow.id.clone(), step_number));
    //     }
    //     self.send_chat_request("[continue]".to_string(), None);
    //     Ok(())
    // }
}
