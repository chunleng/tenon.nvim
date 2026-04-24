use crate::{
    clients::{BehaviorSource, ChatAgent, StreamItem, SupportedModels, get_agent},
    get_application_config,
    tools::resolve_tools,
    utils::GLOBAL_EXECUTION_HANDLER,
};
use chrono::Local;
use nvim_oxi::{Result as OxiResult, api::types::LogLevel};
use rig::{
    OneOrMany,
    agent::Text,
    completion::Usage,
    message::{AssistantContent, Image, Message, ToolResult, ToolResultContent, UserContent},
    tool::ToolDyn,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::LinkedList,
    sync::atomic::{AtomicBool, Ordering},
    sync::{Arc, LazyLock, Mutex, RwLock},
};

pub mod history;

use history::save_to_history;

pub static CHAT_PROCESSES: LazyLock<Mutex<Vec<Arc<RwLock<ChatProcess>>>>> =
    LazyLock::new(|| Mutex::new(Vec::new()));

/// Returns the chat process at `index`, creating new ones as needed.
pub fn get_or_create_chat_process(index: usize) -> Arc<RwLock<ChatProcess>> {
    let mut processes = CHAT_PROCESSES.lock().unwrap();
    while processes.len() <= index {
        processes.push(Arc::new(RwLock::new(ChatProcess::new())));
    }
    processes[index].clone()
}

/// Removes the chat process at `index`, shifting subsequent indices down.
pub fn remove_chat_process(index: usize) {
    let mut processes = CHAT_PROCESSES.lock().unwrap();
    if index < processes.len() {
        processes.remove(index);
    }
}

/// Returns the current number of chat processes.
pub fn chat_process_count() -> usize {
    CHAT_PROCESSES.lock().unwrap().len()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TenonUserTextMessage(pub String);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TenonUserMessage {
    Text(TenonUserTextMessage),
}

impl From<TenonUserMessage> for Message {
    fn from(value: TenonUserMessage) -> Self {
        match value {
            TenonUserMessage::Text(TenonUserTextMessage(msg)) => Message::User {
                content: OneOrMany::one(UserContent::text(msg)),
            },
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TenonAssistantMessageContent {
    Text(String),
}
impl From<TenonAssistantMessageContent> for AssistantContent {
    fn from(value: TenonAssistantMessageContent) -> Self {
        match value {
            TenonAssistantMessageContent::Text(s) => AssistantContent::text(s),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TenonAssistantMessage {
    pub reasoning: Option<String>,
    pub content: Vec<TenonAssistantMessageContent>,
}

impl From<TenonAssistantMessage> for Option<Message> {
    fn from(value: TenonAssistantMessage) -> Self {
        // reasoning is not return to consciously reduce context
        Some(Message::Assistant {
            id: None,
            content: OneOrMany::many(value.content.into_iter().map(|x| x.into())).ok()?,
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TenonToolCall {
    pub id: String,
    pub internal_call_id: String,
    pub name: String,
    pub args: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TenonToolResult {
    Text(Text),
    Image(Image),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TenonToolError(pub String);

impl TenonToolError {
    /// Strip rig's internal wrapping prefixes for display.
    /// E.g. "Toolset error: ToolCallError: ToolCallError: read_file ..."
    ///   → "read_file ..."
    pub fn display_message(&self) -> &str {
        let mut s = self.0.strip_prefix("Toolset error: ").unwrap_or(&self.0);
        while let Some(stripped) = s.strip_prefix("ToolCallError: ") {
            s = stripped;
        }
        s
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TenonToolLog {
    pub tool_call: TenonToolCall,
    pub tool_result: Option<Result<TenonToolResult, TenonToolError>>,
}

impl From<TenonToolLog> for Vec<Message> {
    fn from(value: TenonToolLog) -> Self {
        let mut messages = vec![Message::Assistant {
            id: None,
            content: OneOrMany::one(AssistantContent::tool_call(
                value.tool_call.id.clone(),
                value.tool_call.name,
                value.tool_call.args,
            )),
        }];
        if let Some(res) = value.tool_result {
            let tool_result_content = match &res {
                Ok(TenonToolResult::Text(text)) => {
                    OneOrMany::one(ToolResultContent::Text(text.clone()))
                }
                Ok(TenonToolResult::Image(img)) => {
                    OneOrMany::one(ToolResultContent::Image(img.clone()))
                }
                Err(err) => OneOrMany::one(ToolResultContent::text(&err.0)),
            };
            messages.push(Message::User {
                content: OneOrMany::one(UserContent::ToolResult(ToolResult {
                    id: value.tool_call.id,
                    call_id: None,
                    content: tool_result_content,
                })),
            });
        }

        messages
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TenonLog {
    User(TenonUserMessage),
    Assistant(TenonAssistantMessage),
    Tool(TenonToolLog),
}

impl From<TenonLog> for Vec<Message> {
    fn from(value: TenonLog) -> Self {
        match value {
            TenonLog::User(user_message) => vec![user_message.into()],
            TenonLog::Assistant(assistant_message) => {
                match Option::<Message>::from(assistant_message) {
                    Some(x) => vec![x.into()],
                    None => vec![],
                }
            }
            TenonLog::Tool(tool_log) => tool_log.into(),
        }
    }
}

fn generate_chat_id() -> String {
    let now = Local::now();
    let date = now.format("%Y-%m-%d");
    let hash = format!("{:08x}", now.timestamp_subsec_nanos());
    format!("{}_{}", date, hash)
}

pub struct ChatProcess {
    pub id: String,
    pub logs: Arc<RwLock<LinkedList<TenonLog>>>,
    pub usage: Arc<RwLock<Option<Usage>>>,
    pub active_agent: ActiveAgent,
    cancel_token: Arc<AtomicBool>,
    active_thread: Option<std::thread::JoinHandle<()>>,
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

impl ChatProcess {
    pub fn new() -> Self {
        Self::with_agent_name(get_application_config().default_agent)
            .expect("the program failed to enforce default_agent validation")
    }

    pub fn with_agent_name(agent_name: String) -> OxiResult<Self> {
        Ok(Self {
            id: generate_chat_id(),
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
        })
    }

    pub fn cancel(&mut self) {
        self.cancel_token.store(true, Ordering::SeqCst);
    }

    pub fn is_processing(&self) -> bool {
        if let Some(thread) = self.active_thread.as_ref() {
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

        let logs_clone = Arc::clone(&self.logs);
        let usage_clone = Arc::clone(&self.usage);
        let agent_clone = self.active_agent.clone();
        let chat_id = self.id.clone();
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
                            save_to_history(
                                &chat_id,
                                &agent_clone.name,
                                &agent_clone.inner.model.display_name(),
                                &logs_clone,
                                &usage_clone,
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
