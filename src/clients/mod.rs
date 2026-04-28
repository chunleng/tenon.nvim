mod anthropic;
mod bedrock;
mod gemini;
mod ollama;
mod openai;

use rig::{
    agent::Agent,
    message::Message,
    providers::{
        anthropic as rig_anthropic, gemini as rig_gemini, ollama as rig_ollama,
        openai as rig_openai,
    },
    streaming::StreamingChat,
    tool::ToolDyn,
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

pub use anthropic::{AnthropicProviderConfig, get_anthropic_agent};
pub use bedrock::get_bedrock_agent;
pub use gemini::{GeminiProviderConfig, get_gemini_agent};
pub use ollama::{OllamaProviderConfig, get_ollama_agent};
pub use openai::{OpenAIProviderConfig, get_openai_agent};

/// Describes where a behavior comes from: an inline string or a file path.
///
/// When `BehaviorSource::File` is used with a relative path, it is resolved
/// relative to the current working directory.
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase", deny_unknown_fields)]
pub enum BehaviorSource {
    /// An inline string.
    Text { value: String },
    /// A file path. Relative paths are resolved against the current working directory.
    File { path: std::path::PathBuf },
}

impl BehaviorSource {
    /// Resolve the source into its final string content.
    ///
    /// For `Text` this returns the value directly.
    /// For `File` this reads the file contents, resolving relative paths against CWD.
    pub fn resolve(&self) -> Result<String, std::io::Error> {
        match self {
            BehaviorSource::Text { value } => Ok(value.clone()),
            BehaviorSource::File { path } => {
                let resolved = if path.is_absolute() {
                    path.clone()
                } else {
                    std::env::current_dir()?.join(path)
                };
                if resolved.exists() {
                    std::fs::read_to_string(&resolved)
                } else {
                    Ok("".to_string())
                }
            }
        }
    }
}

#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct SupportedModels {
    pub config: ProviderConfig,
    pub model_name: String,
}

impl SupportedModels {
    pub fn display_name(&self) -> String {
        match self.config {
            ProviderConfig::Ollama(_) => format!("ollama: {}", self.model_name),
            ProviderConfig::Gemini(_) => format!("gemini: {}", self.model_name),
            ProviderConfig::OpenAI(_) => format!("openai: {}", self.model_name),
            ProviderConfig::Anthropic(_) => format!("anthropic: {}", self.model_name),
            ProviderConfig::Bedrock(_) => format!("bedrock: {}", self.model_name),
        }
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

pub enum ChatAgent {
    Ollama(Agent<rig_ollama::CompletionModel>),
    Gemini(Agent<rig_gemini::CompletionModel>),
    OpenAI(Agent<rig_openai::CompletionModel>),
    Anthropic(Agent<rig_anthropic::completion::CompletionModel>),
    Bedrock(Agent<rig_bedrock::completion::CompletionModel>),
}

pub enum ChatStream {
    Ollama(
        futures::stream::BoxStream<
            'static,
            Result<
                rig::agent::MultiTurnStreamItem<rig_ollama::StreamingCompletionResponse>,
                rig::agent::StreamingError,
            >,
        >,
    ),
    Gemini(
        futures::stream::BoxStream<
            'static,
            Result<
                rig::agent::MultiTurnStreamItem<rig_gemini::streaming::StreamingCompletionResponse>,
                rig::agent::StreamingError,
            >,
        >,
    ),
    OpenAI(
        futures::stream::BoxStream<
            'static,
            Result<
                rig::agent::MultiTurnStreamItem<rig_openai::streaming::StreamingCompletionResponse>,
                rig::agent::StreamingError,
            >,
        >,
    ),
    Anthropic(
        futures::stream::BoxStream<
            'static,
            Result<
                rig::agent::MultiTurnStreamItem<
                    rig_anthropic::streaming::StreamingCompletionResponse,
                >,
                rig::agent::StreamingError,
            >,
        >,
    ),
    Bedrock(
        futures::stream::BoxStream<
            'static,
            Result<
                rig::agent::MultiTurnStreamItem<rig_bedrock::streaming::BedrockStreamingResponse>,
                rig::agent::StreamingError,
            >,
        >,
    ),
}

pub enum StreamItem {
    ToolResult {
        tool_result: rig::message::ToolResult,
        internal_call_id: String,
    },
    ReasoningDelta {
        reasoning: String,
    },
    Text {
        text: String,
    },
    ToolCall {
        tool_call: rig::message::ToolCall,
        internal_call_id: String,
    },
    Final {
        token_usage: Option<rig::completion::Usage>,
    },
    Other,
}

macro_rules! convert_stream_item {
    ($item:expr) => {{
        use rig::agent::MultiTurnStreamItem;
        use rig::completion::GetTokenUsage;
        use rig::streaming::{StreamedAssistantContent, StreamedUserContent};

        match $item {
            MultiTurnStreamItem::StreamUserItem(StreamedUserContent::ToolResult {
                tool_result,
                internal_call_id,
                ..
            }) => StreamItem::ToolResult {
                tool_result,
                internal_call_id,
            },
            MultiTurnStreamItem::StreamAssistantItem(
                StreamedAssistantContent::ReasoningDelta { reasoning, .. },
            ) => StreamItem::ReasoningDelta { reasoning },
            MultiTurnStreamItem::StreamAssistantItem(StreamedAssistantContent::Text(
                text_struct,
            )) => StreamItem::Text {
                text: text_struct.text,
            },
            MultiTurnStreamItem::StreamAssistantItem(StreamedAssistantContent::ToolCall {
                tool_call,
                internal_call_id,
            }) => StreamItem::ToolCall {
                tool_call: tool_call.into(),
                internal_call_id,
            },
            MultiTurnStreamItem::StreamAssistantItem(StreamedAssistantContent::Final(
                final_response,
            )) => StreamItem::Final {
                token_usage: final_response
                    .token_usage()
                    .map(rig::completion::Usage::from),
            },
            _ => StreamItem::Other,
        }
    }};
}

impl ChatStream {
    pub async fn next(&mut self) -> Option<Result<StreamItem, rig::agent::StreamingError>> {
        use futures::stream::StreamExt;
        match self {
            ChatStream::Ollama(stream) => stream.next().await.map(|result| match result {
                Ok(item) => Ok(convert_stream_item!(item)),
                Err(e) => Err(e),
            }),
            ChatStream::Gemini(stream) => stream.next().await.map(|result| match result {
                Ok(item) => Ok(convert_stream_item!(item)),
                Err(e) => Err(e),
            }),
            ChatStream::OpenAI(stream) => stream.next().await.map(|result| match result {
                Ok(item) => Ok(convert_stream_item!(item)),
                Err(e) => Err(e),
            }),
            ChatStream::Anthropic(stream) => stream.next().await.map(|result| match result {
                Ok(item) => Ok(convert_stream_item!(item)),
                Err(e) => Err(e),
            }),
            ChatStream::Bedrock(stream) => stream.next().await.map(|result| match result {
                Ok(item) => Ok(convert_stream_item!(item)),
                Err(e) => Err(e),
            }),
        }
    }
}

impl ChatAgent {
    pub async fn stream_chat(&self, message: String, history: Vec<Message>) -> ChatStream {
        let multi_turn = 30;
        match self {
            ChatAgent::Ollama(agent) => ChatStream::Ollama(
                agent
                    .stream_chat(message, history)
                    .multi_turn(multi_turn)
                    .await,
            ),
            ChatAgent::Gemini(agent) => ChatStream::Gemini(
                agent
                    .stream_chat(message, history)
                    .multi_turn(multi_turn)
                    .await,
            ),
            ChatAgent::OpenAI(agent) => ChatStream::OpenAI(
                agent
                    .stream_chat(message, history)
                    .multi_turn(multi_turn)
                    .await,
            ),
            ChatAgent::Anthropic(agent) => ChatStream::Anthropic(
                agent
                    .stream_chat(message, history)
                    .multi_turn(multi_turn)
                    .await,
            ),
            ChatAgent::Bedrock(agent) => ChatStream::Bedrock(
                agent
                    .stream_chat(message, history)
                    .multi_turn(multi_turn)
                    .await,
            ),
        }
    }

    /// Non-streaming convenience: collects all text from a single-turn chat.
    /// Ignores tool calls — intended for lightweight sub-agent use (e.g. summarization).
    pub async fn chat(&self, message: String) -> Result<String, rig::agent::StreamingError> {
        let mut stream = self.stream_chat(message, vec![]).await;
        let mut full_text = String::new();
        let mut was_text = false;
        while let Some(result) = stream.next().await {
            match result {
                Ok(StreamItem::Text { text }) => {
                    if !was_text {
                        // Only take the last block of StreamItem::Text to prevent getting LLM's
                        // thought
                        was_text = true;
                        full_text = String::new();
                    }
                    full_text.push_str(&text);
                }
                Ok(_) => was_text = false,
                Err(e) => return Err(e),
            }
        }
        Ok(full_text)
    }
}

pub fn get_agent(
    model: SupportedModels,
    behavior: Vec<BehaviorSource>,
    tools: Vec<Box<dyn ToolDyn>>,
) -> ChatAgent {
    let resolved_behavior = if behavior.is_empty() {
        None
    } else {
        Some(
            behavior
                .into_iter()
                .map(|p| p.resolve())
                .collect::<Result<Vec<_>, _>>()
                .expect("failed to resolve behavior")
                .join("\n"),
        )
    };
    match model.config {
        ProviderConfig::Ollama(config) => ChatAgent::Ollama(get_ollama_agent(
            config,
            model.model_name,
            resolved_behavior,
            tools,
        )),
        ProviderConfig::Gemini(config) => ChatAgent::Gemini(get_gemini_agent(
            config,
            model.model_name,
            resolved_behavior,
            tools,
        )),
        ProviderConfig::OpenAI(config) => ChatAgent::OpenAI(get_openai_agent(
            config,
            model.model_name,
            resolved_behavior,
            tools,
        )),
        ProviderConfig::Anthropic(config) => ChatAgent::Anthropic(get_anthropic_agent(
            config,
            model.model_name,
            resolved_behavior,
            tools,
        )),
        ProviderConfig::Bedrock(_config) => ChatAgent::Bedrock(get_bedrock_agent(
            model.model_name,
            resolved_behavior,
            tools,
        )),
    }
}
