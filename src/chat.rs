use nvim_oxi::{
    Dictionary,
    api::{notify, types::LogLevel},
};
use reqwest::header::{AUTHORIZATION, HeaderMap, HeaderValue};
use rig::{
    client::{CompletionClient, Nothing},
    completion::Chat,
    providers::ollama,
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

                match agent.chat(message, vec![]).await {
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
