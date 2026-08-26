mod anthropic;
mod bedrock;
mod gemini;
mod ollama;
mod openai;

use rig::agent::{
    AgentHook, HookContext, InvalidToolCallAction, InvalidToolCallContext, StepEventKind,
    ToolResultAction, ToolResultEvent,
};
use serde::Deserialize;

/// API key that can be either a direct value or an environment variable reference.
///
/// Supports two formats in configuration:
/// - Direct string: `api_key = "sk-..."`
/// - Env reference: `api_key = { env = "MY_API_KEY" }`
#[derive(Debug, Clone, Deserialize, PartialEq)]
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

/// Prefixes tool error and denied results with `ToolCallError: ` so Tenon's chat
/// handler classifies them as errors. In rig 0.40.0's structured execution path,
/// arg parse failures and some tool errors produce result text without this
/// prefix (e.g. `failed to parse tool arguments: ...`), causing the chat handler
/// to misclassify them as successful results.
pub struct ToolErrorHook;

impl AgentHook for ToolErrorHook {
    fn observes(&self, kind: StepEventKind) -> bool {
        kind == StepEventKind::ToolResult
    }

    async fn on_tool_result(
        &self,
        _ctx: &HookContext,
        event: ToolResultEvent<'_>,
    ) -> ToolResultAction {
        let is_error = event.raw_result.is_error() || event.raw_result.is_refused();
        let presentation_text = event.presentation.as_text().unwrap_or("");
        if is_error && !presentation_text.starts_with("ToolCallError: ") {
            return ToolResultAction::rewrite(format!("ToolCallError: {presentation_text}"));
        }
        ToolResultAction::keep()
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
    pub default_parameters: serde_json::Map<String, serde_json::Value>,
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
    #[serde(rename = "openai_completion")]
    OpenAICompletion(OpenAIProviderConfig),
    #[serde(rename = "openai_response")]
    OpenAIResponse(OpenAIProviderConfig),
    Anthropic(AnthropicProviderConfig),
    Bedrock(NoProviderConfig),
}

#[cfg(test)]
mod tests {
    use super::{InvalidToolCallHook, ToolErrorHook};
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

    /// When the LLM calls a valid tool with invalid arguments (e.g. missing a
    /// required field), the tool result delivered to the model and shown in the
    /// UI must be prefixed with `ToolCallError: ` so Tenon's chat handler
    /// classifies it as an error. In rig 0.40.0's structured execution path, arg
    /// parse failures produce `failed to parse tool arguments: ...` without the
    /// prefix, causing the error to be misclassified as a successful tool result.
    #[tokio::test]
    async fn invalid_tool_args_yield_toolcallerror_prefix() {
        let model = MockCompletionModel::from_stream_turns([
            vec![
                // Call `add` with missing `y` field
                MockStreamEvent::tool_call("tool_call_1", "add", serde_json::json!({"x": 1})),
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
            .add_hook(ToolErrorHook)
            .build();

        let mut stream = agent.stream_prompt("add x and y").max_turns(3).await;

        let mut tool_result_text: Option<String> = None;

        while let Some(item) = stream.next().await {
            match item {
                Ok(MultiTurnStreamItem::StreamUserItem(StreamedUserContent::ToolResult {
                    tool_result,
                    ..
                })) => {
                    if let Some(rig::message::ToolResultContent::Text(text)) =
                        tool_result.content.first()
                    {
                        tool_result_text = Some(text.text.clone());
                    }
                }
                Ok(MultiTurnStreamItem::FinalResponse(_)) => break,
                Ok(_) => {}
                Err(_) => break,
            }
        }

        let text = tool_result_text.expect("should have received a tool result");
        assert!(
            text.starts_with("ToolCallError: "),
            "tool result for invalid args should be prefixed with 'ToolCallError: ' so the \
             chat handler classifies it as an error, but got: {}",
            text
        );
    }

    /// When both `InvalidToolCallHook` and `ToolErrorHook` are registered, an
    /// unknown tool call skipped by `InvalidToolCallHook` (which already prefixes
    /// with `ToolCallError: `) must NOT be double-prefixed by `ToolErrorHook`.
    #[tokio::test]
    async fn skipped_tool_result_not_double_prefixed() {
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
            .add_hook(ToolErrorHook)
            .build();

        let mut stream = agent
            .stream_prompt("use record_workflow")
            .max_turns(3)
            .await;

        let mut tool_result_text: Option<String> = None;

        while let Some(item) = stream.next().await {
            match item {
                Ok(MultiTurnStreamItem::StreamUserItem(StreamedUserContent::ToolResult {
                    tool_result,
                    ..
                })) => {
                    if let Some(rig::message::ToolResultContent::Text(text)) =
                        tool_result.content.first()
                    {
                        tool_result_text = Some(text.text.clone());
                    }
                }
                Ok(MultiTurnStreamItem::FinalResponse(_)) => break,
                Ok(_) => {}
                Err(_) => break,
            }
        }

        let text = tool_result_text.expect("should have received a tool result");
        let prefix_count = text.matches("ToolCallError: ").count();
        assert_eq!(
            prefix_count, 1,
            "skipped tool result should have exactly one 'ToolCallError: ' prefix, not \
             double-prefixed. Got: {}",
            text
        );
    }
}
