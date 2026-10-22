use crate::clients::ApiKey;
use rig::{
    agent::{Agent, AgentBuilder},
    providers::ollama::OllamaConfig,
    tool::DynamicTool,
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
    tools: Vec<DynamicTool>,
    mut params: serde_json::Map<String, serde_json::Value>,
) -> Agent {
    let mut ollama_config = OllamaConfig::new().with_base_url(config.base_url);
    if let Some(bearer) = config.bearer
        && let Ok(token) = bearer.resolve()
    {
        ollama_config = ollama_config.with_api_key(token);
    }
    let model = ollama_config.client().completion(model_name);
    let mut agent = AgentBuilder::new(model);
    if let Some(p) = preamble {
        agent = agent.preamble(&p);
    }
    if let Some(max_tokens) = params.remove("max_tokens").and_then(|v| v.as_u64()) {
        agent = agent.max_tokens(max_tokens);
    }
    if !params.is_empty() {
        agent = agent.additional_params(serde_json::Value::Object(params));
    }

    agent
        .dynamic_tools(tools)
        .add_hook(crate::clients::InvalidToolCallHook)
        .build()
}
