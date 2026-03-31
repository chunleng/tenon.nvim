use nvim_oxi::{
    Dictionary,
    api::{notify, types::LogLevel},
};
use reqwest::header::{AUTHORIZATION, HeaderMap, HeaderValue};
use rig::{
    OneOrMany,
    agent::Text,
    client::{CompletionClient, Nothing},
    completion::{self, Chat},
    message::{self, UserContent},
    providers::ollama::{self},
};
use std::{
    collections::LinkedList,
    sync::{Arc, RwLock},
};

pub struct ChatProcess {
    pub logs: Arc<RwLock<LinkedList<ollama::Message>>>,
}

impl ChatProcess {
    pub fn new() -> Self {
        Self {
            logs: Arc::new(RwLock::new(LinkedList::new())),
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
                let agent = client.agent("gemini-3-flash-preview").build();

                let chat_history;
                if let Ok(logs) = logs_clone.read() {
                    chat_history = logs
                        .iter()
                        .cloned()
                        .filter_map(|x| match x {
                            ollama::Message::Assistant { content, .. } => {
                                Some(completion::Message::Assistant {
                                    id: None,
                                    content: OneOrMany::one(message::AssistantContent::text(
                                        content,
                                    )),
                                })
                            }
                            ollama::Message::User { content, .. } => {
                                Some(completion::Message::User {
                                    content: OneOrMany::one(UserContent::Text(Text::from(content))),
                                })
                            }
                            ollama::Message::ToolResult { .. } => None,
                            ollama::Message::System { content, .. } => {
                                Some(completion::Message::System { content })
                            }
                        })
                        .collect::<Vec<_>>();
                } else {
                    todo!("fix after error is introduced")
                }

                match agent.chat(message, chat_history).await {
                    Ok(response) => {
                        if let Ok(mut logs) = logs_clone.write() {
                            logs.push_back(ollama::Message::Assistant {
                                content: response,
                                images: None,
                                name: None,
                                thinking: None,
                                tool_calls: vec![],
                            });
                        }
                    }
                    Err(e) => {
                        let _ = notify(
                            &format!("Error: {}", e),
                            LogLevel::Error,
                            &Dictionary::new(),
                        );
                    }
                }
            });
        });
    }
}
