use crate::tools::ReadFile;
use futures::stream::StreamExt;
use nvim_oxi::{
    Dictionary,
    api::{notify, types::LogLevel},
};
use reqwest::header::{AUTHORIZATION, HeaderMap, HeaderValue};
use rig::{
    OneOrMany,
    agent::{MultiTurnStreamItem, Text},
    client::{CompletionClient, Nothing},
    completion::{self, GetTokenUsage, Usage},
    message::{self, ToolResult, ToolResultContent, UserContent},
    providers::ollama::{self, ToolCall},
    streaming::{StreamedAssistantContent, StreamedUserContent, StreamingChat},
};
use std::{
    collections::{HashMap, LinkedList},
    sync::{Arc, RwLock},
};

pub struct ChatProcess {
    pub logs: Arc<RwLock<LinkedList<ollama::Message>>>,
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
            logs.push_back(ollama::Message::User {
                content: message.clone(),
                images: None,
                name: None,
            });
        }

        let logs_clone = Arc::clone(&self.logs);
        let usage_clone = Arc::clone(&self.usage);
        std::thread::spawn(move || {
            let rt = tokio::runtime::Runtime::new().unwrap();
            rt.block_on(async {
                let mut headers = HeaderMap::new();
                headers.insert(
                    AUTHORIZATION,
                    HeaderValue::from_str(&format!(
                        "Bearer {}",
                        std::env::var("OLLAMA_API_KEY").expect("OLLAMA_API_KEY must be set")
                    ))
                    .unwrap(),
                );
                let client = ollama::Client::builder()
                    .base_url("https://ollama.com")
                    .http_headers(headers)
                    .api_key(Nothing)
                    .build()
                    .unwrap();
                let agent = client
                    // TODO https://github.com/ollama/ollama/issues/14567
                    // gemini-3-flash-preview tools does not work because it requires additional
                    // `thought_signature`
                    // .agent("gemini-3-flash-preview")
                    .agent("glm-5")
                    .tool(ReadFile)
                    .build();

                let chat_history;
                if let Ok(logs) = logs_clone.read() {
                    chat_history = logs
                        .iter()
                        .cloned()
                        .filter_map(|x| match x {
                            ollama::Message::Assistant {
                                content,
                                tool_calls,
                                ..
                            } => {
                                let content = match content.is_empty() {
                                    true => OneOrMany::many(tool_calls.iter().map(|x| {
                                        message::AssistantContent::tool_call(
                                            "".to_string(),
                                            x.function.name.clone(),
                                            x.function.arguments.clone(),
                                        )
                                    })),
                                    false => {
                                        OneOrMany::many([message::AssistantContent::text(content)])
                                    }
                                }
                                .ok()?;
                                Some(completion::Message::Assistant { id: None, content })
                            }
                            ollama::Message::User { content, .. } => {
                                Some(completion::Message::User {
                                    content: OneOrMany::one(UserContent::Text(Text::from(content))),
                                })
                            }
                            ollama::Message::ToolResult { content, .. } => {
                                Some(completion::Message::User {
                                    content: OneOrMany::one(UserContent::ToolResult(ToolResult {
                                        id: "".to_string(),
                                        call_id: None,
                                        content: OneOrMany::one(message::ToolResultContent::text(
                                            content,
                                        )),
                                    })),
                                })
                            }
                            ollama::Message::System { content, .. } => {
                                Some(completion::Message::System { content })
                            }
                        })
                        .collect::<Vec<_>>();
                } else {
                    todo!("fix after error is introduced")
                }

                let mut stream = agent.stream_chat(message, chat_history).multi_turn(3).await;
                let mut full_response = String::new();
                if let Ok(mut logs) = logs_clone.write() {
                    logs.push_back(ollama::Message::Assistant {
                        content: full_response.clone(),
                        images: None,
                        name: None,
                        thinking: None,
                        tool_calls: vec![],
                    });
                }
                let mut tools_lookup: HashMap<String, ToolCall> = HashMap::new();
                while let Some(chunk) = stream.next().await {
                    match chunk {
                        Ok(MultiTurnStreamItem::StreamUserItem(
                            StreamedUserContent::ToolResult {
                                tool_result,
                                internal_call_id,
                            },
                        )) => {
                            if let Ok(mut logs) = logs_clone.write() {
                                logs.push_back(ollama::Message::ToolResult {
                                    name: tools_lookup
                                        .get(&internal_call_id)
                                        .map(|x| x.function.name.clone())
                                        .unwrap_or("unknown tool".to_string()),
                                    content: tool_result
                                        .content
                                        .into_iter()
                                        .filter_map(|x| match x {
                                            ToolResultContent::Text(Text { text }) => {
                                                Some(text.to_string())
                                            }
                                            _ => None,
                                        })
                                        .collect::<Vec<_>>()
                                        .join("\n"),
                                });
                                tools_lookup = HashMap::new();
                                full_response = "".to_string();
                                logs.push_back(ollama::Message::Assistant {
                                    content: full_response.clone(),
                                    images: None,
                                    name: None,
                                    thinking: None,
                                    tool_calls: vec![],
                                });
                            }
                        }
                        Ok(MultiTurnStreamItem::StreamAssistantItem(
                            StreamedAssistantContent::Text(text_struct),
                        )) => {
                            full_response.push_str(&text_struct.text);
                            if let Ok(mut logs) = logs_clone.write() {
                                // TODO make this more efficient
                                logs.pop_back();
                                logs.push_back(ollama::Message::Assistant {
                                    content: full_response.clone(),
                                    images: None,
                                    name: None,
                                    thinking: None,
                                    tool_calls: tools_lookup.values().cloned().collect::<Vec<_>>(),
                                });
                            }
                        }
                        Ok(MultiTurnStreamItem::StreamAssistantItem(
                            StreamedAssistantContent::ToolCall {
                                tool_call,
                                internal_call_id,
                            },
                        )) => {
                            tools_lookup.insert(internal_call_id, tool_call.into());
                            if let Ok(mut logs) = logs_clone.write() {
                                logs.pop_back();
                                logs.push_back(ollama::Message::Assistant {
                                    content: full_response.clone(),
                                    images: None,
                                    name: None,
                                    thinking: None,
                                    tool_calls: tools_lookup.values().cloned().collect::<Vec<_>>(),
                                });
                            }
                        }
                        Ok(MultiTurnStreamItem::StreamAssistantItem(
                            StreamedAssistantContent::Final(final_response),
                        )) => {
                            if let Some(usage) = final_response.token_usage() {
                                if let Ok(mut usage_lock) = usage_clone.write() {
                                    *usage_lock = Some(usage);
                                }
                            }
                        }
                        Ok(_) => {}
                        Err(e) => {
                            let lines = format!("{}", e)
                                .lines()
                                .map(|x| x.to_string())
                                .collect::<Vec<String>>();
                            for line in lines {
                                let _ = notify(&line, LogLevel::Error, &Dictionary::new());
                            }
                            break;
                        }
                    }
                }
            });
        });
    }
}
