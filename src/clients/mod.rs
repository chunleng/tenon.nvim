mod anthropic;
mod bedrock;
mod gemini;
mod ollama;
mod openai;

use rig::completion::CompletionModel;
use rig::agent::{AgentHook, Flow, HookContext, StepEvent, StepEventKind};
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

impl<M> AgentHook<M> for InvalidToolCallHook
where
    M: CompletionModel,
{
    fn observes(&self, kind: StepEventKind) -> bool {
        kind == StepEventKind::InvalidToolCall
    }

    async fn on_event(&self, _ctx: &HookContext, event: StepEvent<'_, M>) -> Flow {
        if let StepEvent::InvalidToolCall(ctx) = event {
            return Flow::skip(format!(
                "ToolCallError: `{}` is not an available tool. Call one of: {}.",
                ctx.tool_name,
                ctx.available_tools.join(", ")
            ));
        }
        Flow::cont()
    }
}

pub use anthropic::{AnthropicProviderConfig, get_anthropic_agent};
pub use bedrock::get_bedrock_agent;
pub use gemini::{GeminiProviderConfig, get_gemini_agent};
pub use ollama::{OllamaProviderConfig, get_ollama_agent};
pub use openai::{OpenAIProviderConfig, get_openai_agent};

#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct SupportedModels {
    pub connector_name: String,
    pub config: ProviderConfig,
    pub model_name: String,
}

impl SupportedModels {
    pub fn display_name(&self) -> String {
        format!("{}: {}", self.connector_name, self.model_name)
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct NoProviderConfig;

#[allow(dead_code)]
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase", deny_unknown_fields)]
pub enum ProviderConfig {
    Ollama(OllamaProviderConfig),
    Gemini(GeminiProviderConfig),
    OpenAI(OpenAIProviderConfig),
    Anthropic(AnthropicProviderConfig),
    Bedrock(NoProviderConfig),
}

#[cfg(test)]
mod tests {
    use super::InvalidToolCallHook;
    use futures::StreamExt;
    use rig::agent::{AgentBuilder, MultiTurnStreamItem, StreamingError};
    use rig::prelude::StreamingPrompt;
    use rig::streaming::{StreamedAssistantContent, StreamedUserContent};
    use rig::test_utils::{MockAddTool, MockCompletionModel, MockStreamEvent};

    /// When the LLM calls a tool that is not registered (e.g. `record_workflow`
    /// when only `add` is available), the `InvalidToolCallHook` should treat it as
    /// a recoverable tool failure — emitting a synthetic ToolResult so the model
    /// can continue — rather than terminating the stream with a fatal
    /// `StreamingError::Prompt(PromptError::UnknownToolCall)`.
    #[tokio::test]
    async fn unknown_tool_call_yields_tool_result_not_streaming_error() {
        let model = MockCompletionModel::from_stream_turns([
            vec![
                MockStreamEvent::tool_call("tool_call_1", "record_workflow", serde_json::json!({})),
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

        let mut stream = agent
            .stream_prompt("use record_workflow")
            .max_turns(3)
            .await;

        let mut saw_tool_result = false;
        let mut streaming_error: Option<StreamingError> = None;

        while let Some(item) = stream.next().await {
            match item {
                Ok(MultiTurnStreamItem::StreamUserItem(StreamedUserContent::ToolResult {
                    ..
                })) => {
                    saw_tool_result = true;
                }
                Ok(MultiTurnStreamItem::StreamAssistantItem(StreamedAssistantContent::Final(
                    _,
                ))) => {}
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
}
