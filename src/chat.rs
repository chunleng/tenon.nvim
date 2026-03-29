use nvim_oxi::{libuv::AsyncHandle, print};
use reqwest::header::{AUTHORIZATION, HeaderMap, HeaderValue};
use rig::{
    client::{CompletionClient, Nothing},
    completion::Prompt,
    providers::ollama::{self},
};
use tokio::sync::mpsc;

pub struct Chat;

impl Chat {
    pub fn send_message(message: String) {
        let (tx, mut rx) = mpsc::unbounded_channel::<String>();

        let async_handle = AsyncHandle::new(move || {
            while let Ok(msg) = rx.try_recv() {
                print!("{}", "received");
                print!("{}", msg.lines().next().unwrap().to_string());
            }
        })
        .unwrap();

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
                let agent = client.agent("glm-5").build();
                tx.send(format!(
                    "Message received: {}",
                    agent.prompt(message).await.unwrap()
                ))
                .unwrap();
                async_handle.send().unwrap();
            });
        });
    }
}
