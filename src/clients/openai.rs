use crate::clients::ApiKey;
use rig::{agent::Agent, client::AgentClientExt, providers::openai, tool::DynamicTool};
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

pub fn get_openai_completion_api_agent(
    config: OpenAIProviderConfig,
    model_name: String,
    preamble: Option<String>,
    tools: Vec<DynamicTool>,
    mut params: serde_json::Map<String, serde_json::Value>,
) -> Agent<openai::CompletionModel> {
    let api_key = config.api_key.resolve().unwrap_or_else(|e| {
        crate::utils::GLOBAL_EXECUTION_HANDLER
            .notify_on_main_thread(format!("{}", e), nvim_oxi::api::types::LogLevel::Error);
        String::new()
    });
    let client = openai::Client::builder()
        .base_url(config.base_url)
        .api_key(api_key)
        .build()
        .unwrap()
        .completions_api();
    let mut agent = client.agent(model_name);
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
        .add_hook(crate::clients::ToolErrorHook)
        .build()
}

pub fn get_openai_response_api_agent(
    config: OpenAIProviderConfig,
    model_name: String,
    preamble: Option<String>,
    tools: Vec<DynamicTool>,
    mut params: serde_json::Map<String, serde_json::Value>,
) -> Agent<openai::responses_api::ResponsesCompletionModel> {
    let api_key = config.api_key.resolve().unwrap_or_else(|e| {
        crate::utils::GLOBAL_EXECUTION_HANDLER
            .notify_on_main_thread(format!("{}", e), nvim_oxi::api::types::LogLevel::Error);
        String::new()
    });
    let client = openai::Client::builder()
        .base_url(config.base_url)
        .api_key(api_key)
        .build()
        .unwrap();
    let mut agent = client.agent(model_name);
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
        .add_hook(crate::clients::ToolErrorHook)
        .build()
}
