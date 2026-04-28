use crate::clients::ApiKey;
use rig::{agent::Agent, client::CompletionClient, providers::anthropic, tool::ToolDyn};
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct AnthropicProviderConfig {
    pub base_url: String,
    pub api_key: ApiKey,
}

impl Default for AnthropicProviderConfig {
    fn default() -> Self {
        Self {
            base_url: "https://api.anthropic.com".to_string(),
            api_key: ApiKey::Env {
                env: "ANTHROPIC_API_KEY".to_string(),
            },
        }
    }
}

pub fn get_anthropic_agent(
    config: AnthropicProviderConfig,
    model_name: String,
    preamble: Option<String>,
    tools: Vec<Box<dyn ToolDyn>>,
) -> Agent<anthropic::completion::CompletionModel> {
    let api_key = config.api_key.resolve().unwrap_or_else(|e| {
        eprintln!("[tenon] {}", e);
        String::new()
    });
    let client = anthropic::Client::builder()
        .base_url(config.base_url)
        .api_key(api_key)
        .build()
        .unwrap();
    let mut agent = client.agent(model_name);
    if let Some(p) = preamble {
        agent = agent.preamble(&p);
    }
    let agent = agent
        .max_tokens(16000)
        .additional_params(serde_json::json!({
            "thinking": { "type": "enabled", "budget_tokens": 10000 }
        }))
        .tools(tools)
        .build();

    agent
}
