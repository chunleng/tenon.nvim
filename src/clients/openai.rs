use crate::clients::ApiKey;
use rig::{agent::Agent, client::CompletionClient, providers::openai, tool::ToolDyn};
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct OpenAIProviderConfig {
    pub base_url: String,
    pub api_key: ApiKey,
}

impl Default for OpenAIProviderConfig {
    fn default() -> Self {
        Self {
            base_url: "https://api.openai.com/v1".to_string(),
            api_key: ApiKey::Env {
                env: "OPENAI_API_KEY".to_string(),
            },
        }
    }
}

pub fn get_openai_agent(
    config: OpenAIProviderConfig,
    model_name: String,
    preamble: Option<String>,
    tools: Vec<Box<dyn ToolDyn>>,
) -> Agent<openai::CompletionModel> {
    let api_key = config.api_key.resolve().unwrap_or_else(|e| {
        eprintln!("[tenon] {}", e);
        String::new()
    });
    let client = openai::Client::builder()
        .base_url(config.base_url)
        .api_key(api_key)
        .build()
        .unwrap()
        .completions_api();
    let mut agent = client.agent(model_name.clone());
    if let Some(p) = preamble {
        agent = agent.preamble(&p);
    }
    // non-exhaustive for thinking model for now
    if ["gpt-5.4", "o3", "o1"].contains(&model_name.as_str()) {
        agent = agent.additional_params(serde_json::json!({
            "reasoning_effort": "high"
        }));
    }
    let agent = agent.tools(tools).build();

    agent
}
