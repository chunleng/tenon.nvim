use crate::{
    clients::{OllamaProviderConfig, StreamItem, SupportedModels, get_agent},
    tools::ReadFile,
    utils::GLOBAL_EXECUTION_HANDLER,
};
use rig::{
    OneOrMany,
    completion::Usage,
    message::{AssistantContent, Message, ToolCall, UserContent},
};
use std::{
    collections::{HashMap, LinkedList},
    sync::{Arc, RwLock},
};

pub struct ChatProcess {
    pub logs: Arc<RwLock<LinkedList<Message>>>,
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
            logs.push_back(Message::User {
                content: OneOrMany::one(UserContent::text(message.clone())),
            });
        }

        let logs_clone = Arc::clone(&self.logs);
        let usage_clone = Arc::clone(&self.usage);
        std::thread::spawn(move || {
            let rt = tokio::runtime::Runtime::new().unwrap();
            rt.block_on(async {
                let agent = get_agent(
                    SupportedModels::Ollama {
                        config: OllamaProviderConfig {
                            base_url: "https://ollama.com".to_string(),
                            ..Default::default()
                        },
                        // model_name: "gemini-3-flash-preview".to_string(),
                        model_name: "glm-5.1".to_string(),
                    },
                    Some(
                        "Answer in the fewest words possible. Use abbreviations, symbols, and
                        fragments. Omit articles, conjunctions, and filler. Be precise, not
                        verbose."
                            .to_string(),
                    ),
                    vec![ReadFile],
                );
                let chat_history;
                if let Ok(logs) = logs_clone.read() {
                    chat_history = logs.iter().cloned().collect::<Vec<_>>();
                } else {
                    todo!("fix after error is introduced")
                }

                let mut stream = agent.stream_chat(message, chat_history).await;
                let mut full_response = String::new();
                if let Ok(mut logs) = logs_clone.write() {
                    logs.push_back(Message::Assistant {
                        id: None,
                        content: OneOrMany::one(AssistantContent::text(full_response.clone())),
                    });
                }
                let mut tools_lookup: HashMap<String, ToolCall> = HashMap::new();
                while let Some(result) = stream.next().await {
                    match result {
                        Ok(StreamItem::ToolResult {
                            tool_result,
                            internal_call_id,
                        }) => {
                            if let Ok(mut logs) = logs_clone.write() {
                                logs.push_back(Message::User {
                                    content: OneOrMany::one(UserContent::tool_result_with_call_id(
                                        tool_result.id,
                                        internal_call_id,
                                        tool_result.content,
                                    )),
                                });
                                tools_lookup = HashMap::new();
                                full_response = "".to_string();
                                logs.push_back(Message::Assistant {
                                    id: None,
                                    content: OneOrMany::one(AssistantContent::text(
                                        full_response.clone(),
                                    )),
                                });
                            }
                        }
                        Ok(StreamItem::Text { text }) => {
                            full_response.push_str(&text);
                            if let Ok(mut logs) = logs_clone.write() {
                                logs.pop_back();
                                let mut content =
                                    vec![AssistantContent::text(full_response.clone())];
                                content.extend(
                                    tools_lookup
                                        .values()
                                        .cloned()
                                        .map(|tc: ToolCall| AssistantContent::ToolCall(tc)),
                                );
                                logs.push_back(Message::Assistant {
                                    id: None,
                                    content: OneOrMany::many(content).unwrap(),
                                });
                            }
                        }
                        Ok(StreamItem::ToolCall {
                            tool_call,
                            internal_call_id,
                        }) => {
                            tools_lookup.insert(internal_call_id.clone(), tool_call.into());
                            if let Ok(mut logs) = logs_clone.write() {
                                logs.pop_back();
                                let mut content =
                                    vec![AssistantContent::text(full_response.clone())];
                                content.extend(tools_lookup.values().cloned().map(
                                    move |tc: ToolCall| {
                                        AssistantContent::tool_call(
                                            internal_call_id.clone(),
                                            tc.function.name,
                                            tc.function.arguments,
                                        )
                                    },
                                ));
                                logs.push_back(Message::Assistant {
                                    id: None,
                                    content: OneOrMany::many(content).unwrap(),
                                });
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
                        Err(_) => {
                            // TODO add tracing logs
                            let _ = GLOBAL_EXECUTION_HANDLER.execute_on_main_thread(
                                r#"vim.notify(
                                    "error occurred while streaming response from LLM",
                                    vim.log.levels.ERROR)"#,
                            );
                        }
                    }
                }
            });
        });
    }
}
