use crate::clients::ApiKey;
use rig::{
    agent::{Agent, AgentBuilder},
    providers::anthropic::AnthropicConfig,
    tool::DynamicTool,
};
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
    tools: Vec<DynamicTool>,
    mut params: serde_json::Map<String, serde_json::Value>,
) -> Agent {
    let api_key = config.api_key.resolve().unwrap_or_else(|e| {
        crate::utils::GLOBAL_EXECUTION_HANDLER
            .notify_on_main_thread(e.to_string(), nvim_oxi::api::types::LogLevel::Error);
        String::new()
    });
    let model = AnthropicConfig::new(api_key)
        .with_base_url(config.base_url)
        .client()
        .completion(model_name);
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
