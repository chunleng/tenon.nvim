use crate::{
    clients::{OllamaProviderConfig, StreamItem, SupportedModels, get_agent},
    mcp::McpHubCaller,
    tools::{EditFile, FetchWebpage, ReadFile, WriteFile},
    utils::GLOBAL_EXECUTION_HANDLER,
};
use nvim_oxi::api::types::LogLevel;
use rig::{
    OneOrMany,
    agent::Text,
    completion::Usage,
    message::{AssistantContent, Image, Message, ToolResult, ToolResultContent, UserContent},
    tool::ToolDyn,
    tools::ThinkTool,
};
use serde_json::Value;
use std::{
    collections::LinkedList,
    sync::{Arc, RwLock},
};

#[derive(Debug, Clone)]
pub struct TenonUserTextMessage(pub String);

#[derive(Debug, Clone)]
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

#[derive(Debug, Clone)]
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

#[derive(Debug, Clone)]
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

#[derive(Debug, Clone)]
pub struct TenonToolCall {
    pub id: String,
    pub internal_call_id: String,
    pub name: String,
    pub args: Value,
}

#[derive(Debug, Clone)]
pub enum TenonToolResult {
    Text(Text),
    Image(Image),
}

#[derive(Debug, Clone)]
pub struct TenonToolLog {
    pub tool_call: TenonToolCall,
    pub tool_result: Option<TenonToolResult>,
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
            messages.push(match res {
                TenonToolResult::Text(text) => Message::User {
                    content: OneOrMany::one(UserContent::ToolResult(ToolResult {
                        id: value.tool_call.id,
                        call_id: None,
                        content: OneOrMany::one(ToolResultContent::Text(text)),
                    })),
                },
                TenonToolResult::Image(img) => Message::User {
                    content: OneOrMany::one(UserContent::ToolResult(ToolResult {
                        id: value.tool_call.id,
                        call_id: None,
                        content: OneOrMany::one(ToolResultContent::Image(img)),
                    })),
                },
            });
        }

        messages
    }
}

#[derive(Debug, Clone)]
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

pub struct ChatProcess {
    pub logs: Arc<RwLock<LinkedList<TenonLog>>>,
    pub usage: Arc<RwLock<Option<Usage>>>,
}

impl ChatProcess {
    pub fn new() -> Self {
        Self {
            logs: Arc::new(RwLock::new(LinkedList::new())),
            usage: Arc::new(RwLock::new(None)),
        }
    }

    pub fn send_message(&mut self, message: String) {
        if let Ok(mut logs) = self.logs.write() {
            logs.push_back(TenonLog::User(TenonUserMessage::Text(
                TenonUserTextMessage(message.clone()),
            )))
        }

        let logs_clone = Arc::clone(&self.logs);
        let usage_clone = Arc::clone(&self.usage);
        std::thread::spawn(move || {
            let rt = tokio::runtime::Runtime::new().unwrap();
            rt.block_on(async {
                let mut tools: Vec<Box<dyn ToolDyn>> = vec![
                    Box::new(EditFile),
                    Box::new(FetchWebpage),
                    Box::new(ReadFile),
                    Box::new(WriteFile),
                    Box::new(ThinkTool),
                ];
                if let Ok(x) = McpHubCaller::from_mcp_tools() {
                    tools.append(
                        &mut x
                            .into_iter()
                            .map(|x| Box::new(x) as Box<dyn ToolDyn>)
                            .collect::<Vec<_>>(),
                    );
                }
                let agent = get_agent(
                    SupportedModels::Ollama {
                        config: OllamaProviderConfig {
                            base_url: "https://ollama.com".to_string(),
                            ..Default::default()
                        },
                        model_name: "glm-5.1".to_string(),
                    },
                    Some(
                        "Answer in the fewest words possible. Use abbreviations, symbols, and
                        fragments. Omit articles, conjunctions, and filler. Be precise, not
                        verbose."
                            .to_string(),
                    ),
                    tools,
                );
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

                let mut stream = agent.stream_chat(message, chat_history).await;
                while let Some(result) = stream.next().await {
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
                                            TenonToolResult::Text(text)
                                        }
                                        ToolResultContent::Image(img) => {
                                            TenonToolResult::Image(img)
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
        });
    }
}
