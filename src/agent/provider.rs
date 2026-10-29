use crate::clients::{
    ProviderKind, SupportedModels, get_anthropic_agent, get_bedrock_agent, get_gemini_agent,
    get_ollama_agent, get_openai_completion_api_agent, get_openai_response_api_agent,
};
use crate::directive::Directive;
use rig::agent::{Agent, MultiTurnStreamItem, StreamingResult};
use rig::completion::PromptError;
use rig::message::Message;
use rig::streaming::{Item, StreamEvent};
use rig::tool::{DynamicTool, ToolContext};

pub enum StreamItem {
    ToolResult {
        tool_result: rig::message::ToolResult,
    },
    ReasoningDelta {
        reasoning: String,
    },
    Text {
        text: String,
    },
    #[allow(dead_code)]
    ToolCall {
        tool_call: rig::message::ToolCall,
    },
    CompletionCall {
        usage: rig::completion::Usage,
    },
    Other,
}

fn convert_stream_item(item: MultiTurnStreamItem) -> StreamItem {
    match item {
        MultiTurnStreamItem::ToolResult { tool_result } => StreamItem::ToolResult { tool_result },
        MultiTurnStreamItem::StreamAssistantItem(Item::Event(StreamEvent::Reasoning {
            text,
            ..
        })) => StreamItem::ReasoningDelta { reasoning: text },
        MultiTurnStreamItem::StreamAssistantItem(Item::Event(StreamEvent::Text {
            text, ..
        })) => StreamItem::Text { text },
        MultiTurnStreamItem::ToolCall { tool_call } => StreamItem::ToolCall { tool_call },
        MultiTurnStreamItem::CompletionCall(call) => {
            StreamItem::CompletionCall { usage: call.usage }
        }
        _ => StreamItem::Other,
    }
}

pub struct ChatStream {
    inner: StreamingResult,
}

impl ChatStream {
    pub fn new(
        agent: &Agent,
        message: impl Into<Message> + Send,
        history: Vec<Message>,
        max_turns: usize,
        tool_context: ToolContext,
    ) -> Self {
        let tool_concurrency = 10;
        // Anthropic rejects a request whose `messages` array has no user-role message;
        // rig hoists system messages into the top-level `system` param, so send a
        // system-only prompt as a user message instead.
        let message = match message.into() {
            Message::System { content } => Message::user(content),
            other => other,
        };
        let inner = agent
            .prompt(message)
            .history(history)
            .max_turns(max_turns)
            .tool_concurrency(tool_concurrency)
            .tool_context(tool_context)
            .stream();
        ChatStream { inner }
    }

    #[allow(clippy::result_large_err)]
    pub async fn next(&mut self) -> Option<Result<StreamItem, PromptError>> {
        use futures::stream::StreamExt;
        self.inner
            .next()
            .await
            .map(|result| result.map(convert_stream_item))
    }
}

/// Resolves the request-time parameters for a model, in order:
/// - `ProviderConfig.parameters` defaults are the base
/// - `model.parameters` (the live model state) overrides matching keys
/// - `override_params` is applied last
fn resolve_parameters(
    model: &mut SupportedModels,
    override_params: Option<serde_json::Map<String, serde_json::Value>>,
) -> serde_json::Map<String, serde_json::Value> {
    let mut params: serde_json::Map<String, serde_json::Value> = model
        .config
        .parameters
        .iter()
        .map(|(key, parameter)| (key.clone(), parameter.default_value.clone()))
        .collect();
    for (key, value) in &model.parameters {
        params.insert(key.clone(), value.clone());
    }
    if let Some(overrides) = override_params {
        for (key, value) in overrides {
            params.insert(key.clone(), value.clone());
        }
    }
    model.parameters = params.clone();
    params
}

pub fn get_agent(
    mut model: SupportedModels,
    directive: Vec<Directive>,
    tools: Vec<DynamicTool>,
    override_params: Option<serde_json::Map<String, serde_json::Value>>,
) -> Agent {
    let resolved_directive = if directive.is_empty() {
        None
    } else {
        Some(
            directive
                .into_iter()
                .filter_map(|d| match d.resolve() {
                    Ok(resolved) => Some(resolved),
                    Err(e) => {
                        crate::utils::GLOBAL_EXECUTION_HANDLER.notify_on_main_thread(
                            format!("failed to resolve directive: {}", e),
                            nvim_oxi::api::types::LogLevel::Warn,
                        );
                        None
                    }
                })
                .collect::<Vec<_>>()
                .join("\n"),
        )
    };
    let params = resolve_parameters(&mut model, override_params);
    match model.config.kind {
        ProviderKind::Ollama(config) => {
            get_ollama_agent(config, model.model_name, resolved_directive, tools, params)
        }
        ProviderKind::Gemini(config) => {
            get_gemini_agent(config, model.model_name, resolved_directive, tools, params)
        }
        ProviderKind::OpenAICompletion(config) => get_openai_completion_api_agent(
            config,
            model.model_name,
            resolved_directive,
            tools,
            params,
        ),
        ProviderKind::OpenAIResponse(config) => get_openai_response_api_agent(
            config,
            model.model_name,
            resolved_directive,
            tools,
            params,
        ),
        ProviderKind::Anthropic(config) => {
            get_anthropic_agent(config, model.model_name, resolved_directive, tools, params)
        }
        ProviderKind::Bedrock(_config) => {
            get_bedrock_agent(model.model_name, resolved_directive, tools, params)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::resolve_parameters;
    use crate::clients::{
        OllamaProviderConfig, ParameterConfig, ProviderConfig, ProviderKind, SupportedModels,
    };
    use serde_json::json;
    use std::collections::HashMap;

    fn test_model(live_parameters: serde_json::Map<String, serde_json::Value>) -> SupportedModels {
        let mut provider_parameters = HashMap::new();
        provider_parameters.insert(
            "temperature".to_string(),
            ParameterConfig {
                default_value: json!(0.7),
            },
        );
        provider_parameters.insert(
            "top_k".to_string(),
            ParameterConfig {
                default_value: json!(40),
            },
        );
        SupportedModels {
            connector_name: "ollama".to_string(),
            config: ProviderConfig {
                kind: ProviderKind::Ollama(OllamaProviderConfig::default()),
                parameters: provider_parameters,
            },
            model_name: "llama3".to_string(),
            parameters: live_parameters,
            vision: false,
        }
    }

    #[test]
    fn resolve_parameters_uses_live_state_and_persists_overrides() {
        // Live state starts with an overridden temperature; the provider's
        // top_k default is not shadowed yet.
        let mut live = serde_json::Map::new();
        live.insert("temperature".to_string(), json!(0.9));
        let mut model = test_model(live);

        let resolved = resolve_parameters(&mut model, None);
        assert_eq!(resolved.get("temperature"), Some(&json!(0.9)));
        assert_eq!(resolved.get("top_k"), Some(&json!(40)));
        // Resolved state is written back so the model keeps base-only keys.
        assert_eq!(model.parameters.get("top_k"), Some(&json!(40)));

        let mut overrides = serde_json::Map::new();
        overrides.insert("temperature".to_string(), json!(0.5));
        let resolved = resolve_parameters(&mut model, Some(overrides));
        assert_eq!(resolved.get("temperature"), Some(&json!(0.5)));
        assert_eq!(resolved.get("top_k"), Some(&json!(40)));
        // The override is persisted into the live state for subsequent requests.
        assert_eq!(model.parameters.get("temperature"), Some(&json!(0.5)));
    }
}
