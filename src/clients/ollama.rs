use crate::clients::ApiKey;
use reqwest::header::{AUTHORIZATION, HeaderMap, HeaderValue};
use rig::{
    agent::Agent,
    client::{CompletionClient, Nothing},
    providers::ollama,
    tool::ToolDyn,
};
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct OllamaProviderConfig {
    pub base_url: String,
    pub bearer: Option<ApiKey>,
}

impl Default for OllamaProviderConfig {
    fn default() -> Self {
        Self {
            base_url: "http://127.0.0.1:11434".to_string(),
            bearer: Some(ApiKey::Env {
                env: "OLLAMA_API_KEY".to_string(),
            }),
        }
    }
}

pub fn get_ollama_agent(
    config: OllamaProviderConfig,
    model_name: String,
    preamble: Option<String>,
    tools: Vec<Box<dyn ToolDyn>>,
) -> Agent<ollama::CompletionModel> {
    let mut headers = HeaderMap::new();
    if let Some(bearer) = config.bearer {
        if let Ok(token) = bearer.resolve() {
            headers.insert(
                AUTHORIZATION,
                HeaderValue::from_str(&format!("Bearer {}", token)).unwrap(),
            );
        }
    }
    let client = ollama::Client::builder()
        .base_url(config.base_url)
        .http_headers(headers)
        .api_key(Nothing)
        .build()
        .unwrap();
    let mut agent = client.agent(model_name);
    if let Some(p) = preamble {
        agent = agent.preamble(&p);
    }
    let agent = agent
        .additional_params(serde_json::json!({ "think": true }))
        .tools(tools)
        .build();

    agent
}
