mod anthropic;
mod bedrock;
mod gemini;
mod ollama;
mod openai;

use std::collections::HashMap;

use rig::agent::{
    AgentHook, HookContext, InvalidToolCallAction, InvalidToolCallContext, StepEventKind,
};
use serde::Deserialize;

/// API key that can be either a direct value or an environment variable reference.
///
/// Supports two formats in configuration:
/// - Direct string: `api_key = "sk-..."`
/// - Env reference: `api_key = { env = "MY_API_KEY" }`
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum ApiKey {
    /// Direct API key value.
    Value(String),
    /// Reference to an environment variable.
    Env { env: String },
}

impl ApiKey {
    /// Resolves the API key to its actual value.
    ///
    /// For `Value` variants, returns the string directly.
    /// For `Env` variants, reads from environment and returns error if not set.
    pub fn resolve(&self) -> Result<String, String> {
        match self {
            ApiKey::Value(v) => Ok(v.clone()),
            ApiKey::Env { env } => {
                std::env::var(env).map_err(|_| format!("Environment variable '{}' not set", env))
            }
        }
    }
}

impl Default for ApiKey {
    fn default() -> Self {
        ApiKey::Value(String::new())
    }
}

/// Treats unknown/disallowed tool calls as recoverable tool failures instead of
/// fatal streaming errors (rig 0.40.0 default). When the model calls a tool that
/// is not in the available set, this hook records a synthetic ToolResult telling
/// the model which tools it may use, then continues the agent loop so the model
/// can retry with a valid tool — restoring pre-0.40.0 behavior.
pub struct InvalidToolCallHook;

impl AgentHook for InvalidToolCallHook {
    fn observes(&self, kind: StepEventKind) -> bool {
        kind == StepEventKind::InvalidToolCall
    }

    async fn on_invalid_tool_call(
        &self,
        _ctx: &HookContext,
        event: &InvalidToolCallContext,
    ) -> Option<InvalidToolCallAction> {
        Some(InvalidToolCallAction::skip(format!(
            "ToolCallError: `{}` is not an available tool. Call one of: {}.",
            event.tool_name,
            event.available_tools.join(", ")
        )))
    }
}

pub use anthropic::{AnthropicProviderConfig, get_anthropic_agent};
pub use bedrock::get_bedrock_agent;
pub use gemini::{GeminiProviderConfig, get_gemini_agent};
pub use ollama::{OllamaProviderConfig, get_ollama_agent};
pub use openai::{
    OpenAIProviderConfig, get_openai_completion_api_agent, get_openai_response_api_agent,
};

#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct SupportedModels {
    pub connector_name: String,
    pub config: ProviderConfig,
    pub model_name: String,
    /// Model-level default parameters
    pub parameters: serde_json::Map<String, serde_json::Value>,
    /// Whether the model supports vision (image input).
    pub vision: bool,
}

/// Stores necessary information for calling to provider
#[derive(Debug, Clone)]
pub struct SelectedModel {
    /// ID of the registered model
    pub id: String,

    /// Latest parameter state
    pub parameters: serde_json::Map<String, serde_json::Value>,
}

impl SelectedModel {
    pub fn new(id: impl Into<String>) -> Self {
        let id = id.into();
        let definition = Self::lookup(&id);
        Self::select(id, &definition)
    }

    /// Creates a selection from a registry entry, resolving the request-time parameters once:
    /// provider defaults overlaid by the definition's model-level defaults.
    pub fn select(id: impl Into<String>, definition: &SupportedModels) -> Self {
        let mut parameters: serde_json::Map<String, serde_json::Value> = definition
            .config
            .parameters
            .iter()
            .map(|(key, parameter)| (key.clone(), parameter.default_value.clone()))
            .collect();
        parameters.extend(definition.parameters.clone());
        Self {
            id: id.into(),
            parameters,
        }
    }

    pub fn display_name(&self) -> String {
        self.id.clone()
    }

    /// Resolves the model definition from the registry at use time.
    pub fn definition(&self) -> SupportedModels {
        Self::lookup(&self.id)
    }

    fn lookup(id: &str) -> SupportedModels {
        crate::get_application_config()
            .models
            .get(id)
            .cloned()
            .expect("selected model must exist in the models registry")
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct NoProviderConfig;

#[allow(dead_code)]
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase", deny_unknown_fields)]
pub enum ProviderKind {
    Ollama(OllamaProviderConfig),
    Gemini(GeminiProviderConfig),
    #[serde(rename = "openai_completion")]
    OpenAICompletion(OpenAIProviderConfig),
    #[serde(rename = "openai_response")]
    OpenAIResponse(OpenAIProviderConfig),
    Anthropic(AnthropicProviderConfig),
    Bedrock(NoProviderConfig),
}

#[derive(Debug, Clone, Deserialize)]
pub struct ProviderConfig {
    #[serde(flatten)]
    pub kind: ProviderKind,
    #[serde(default)]
    pub parameters: HashMap<String, ParameterConfig>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ParameterConfig {
    pub default_value: serde_json::Value,

    #[serde(default)]
    pub options: Option<Vec<serde_json::Value>>,
}

#[cfg(test)]
mod tests {
    use super::{InvalidToolCallHook, ProviderConfig, ProviderKind, SupportedModels};
    use futures::StreamExt;
    use rig::agent::{AgentBuilder, MultiTurnStreamItem};
    use rig::completion::PromptError;
    use rig::test_utils::{MockAddTool, MockCompletionModel, MockStreamEvent};

    /// When the LLM calls a tool that is not registered (e.g. `record_choreo`
    /// when only `add` is available), the `InvalidToolCallHook` should treat it as
    /// a recoverable tool failure — emitting a synthetic ToolResult so the model
    /// can continue — rather than terminating the stream with a fatal
    /// `PromptError::UnknownToolCall`.
    #[tokio::test]
    async fn unknown_tool_call_yields_tool_result_not_streaming_error() {
        let model = MockCompletionModel::from_stream_turns([
            vec![
                MockStreamEvent::tool_call("tool_call_1", "record_choreo", serde_json::json!({})),
                MockStreamEvent::final_response_with_default_usage(),
            ],
            vec![
                MockStreamEvent::text("recovered"),
                MockStreamEvent::final_response_with_default_usage(),
            ],
        ]);

        let agent = AgentBuilder::new(model)
            .tool(MockAddTool)
            .add_hook(InvalidToolCallHook)
            .build();

        let mut stream = agent.prompt("use record_choreo").max_turns(3).stream();

        let mut saw_tool_result = false;
        let mut streaming_error: Option<PromptError> = None;

        while let Some(item) = stream.next().await {
            match item {
                Ok(MultiTurnStreamItem::ToolResult { .. }) => {
                    saw_tool_result = true;
                }
                Ok(MultiTurnStreamItem::StreamAssistantItem(_)) => {}
                Ok(MultiTurnStreamItem::FinalResponse(_)) => break,
                Ok(_) => {}
                Err(e) => {
                    streaming_error = Some(e);
                    break;
                }
            }
        }

        assert!(
            saw_tool_result,
            "unknown tool call should yield a synthetic tool result so the model can recover, \
             not a fatal streaming error. Got: {:?}",
            streaming_error
        );
    }

    /// `ProviderConfig` is a struct with the provider enum flattened into it,
    /// and `SupportedModels.config` still holds that struct. Deserialization
    /// must keep consuming the `type` tag through the flatten, both when the
    /// config stands alone and when nested inside `SupportedModels`.
    #[test]
    fn provider_config_deserializes_flattened_kind() {
        let config: ProviderConfig =
            serde_json::from_str(r#"{"type":"ollama","base_url":"http://127.0.0.1:11434"}"#)
                .expect("provider config deserializes");
        match config.kind {
            ProviderKind::Ollama(ollama) => {
                assert_eq!(ollama.base_url, "http://127.0.0.1:11434");
            }
            other => panic!("expected ollama kind, got: {other:?}"),
        }

        let model = SupportedModels {
            connector_name: "test".to_string(),
            config: serde_json::from_str(
                r#"{"type":"ollama","base_url":"http://127.0.0.1:11434"}"#,
            )
            .expect("supported model config deserializes"),
            model_name: "glm-5.1".to_string(),
            parameters: serde_json::Map::new(),
            vision: false,
        };
        match model.config.kind {
            ProviderKind::Ollama(ollama) => {
                assert_eq!(ollama.base_url, "http://127.0.0.1:11434");
            }
            other => panic!("expected ollama kind, got: {other:?}"),
        }
    }

    #[test]
    fn provider_config_parameters_deserializes() {
        let config: ProviderConfig = serde_json::from_str(
            r#"{"type":"ollama","base_url":"http://127.0.0.1:11434","parameters":{"temperature":{"default_value":0.7},"top_k":{"default_value":40}}}"#,
        )
        .expect("provider config with parameters deserializes");
        match &config.kind {
            ProviderKind::Ollama(ollama) => {
                assert_eq!(ollama.base_url, "http://127.0.0.1:11434");
            }
            other => panic!("expected ollama kind, got: {other:?}"),
        }
        assert_eq!(
            config
                .parameters
                .get("temperature")
                .map(|parameter| &parameter.default_value),
            Some(&serde_json::json!(0.7)),
            "parameters must round-trip through deserialization"
        );

        let bare: ProviderConfig =
            serde_json::from_str(r#"{"type":"ollama","base_url":"http://127.0.0.1:11434"}"#)
                .expect("provider config without parameters deserializes");
        assert!(
            bare.parameters.is_empty(),
            "parameters should default to empty, got: {:?}",
            bare.parameters
        );
    }
}
