use crate::clients::{
    ProviderConfig, SupportedModels, get_anthropic_agent, get_bedrock_agent, get_gemini_agent,
    get_ollama_agent, get_openai_agent,
};
use crate::directive::Directive;
use rig::agent::Agent;
use rig::message::Message;
use rig::providers::{
    anthropic as rig_anthropic, gemini as rig_gemini, ollama as rig_ollama, openai as rig_openai,
};
use rig::streaming::StreamingChat;
use rig::tool::ToolDyn;

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
    CompletionCall {
        usage: rig::completion::Usage,
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
                token_usage: Some(final_response.token_usage()),
            },
            MultiTurnStreamItem::CompletionCall(call) => {
                StreamItem::CompletionCall { usage: call.usage }
            }
            MultiTurnStreamItem::FinalResponse(response) => StreamItem::Final {
                token_usage: Some(response.usage),
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
    pub async fn stream_chat(
        &self,
        message: impl Into<Message> + Send,
        history: Vec<Message>,
    ) -> ChatStream {
        let multi_turn = 100;
        let tool_concurrency = 10;
        match self {
            ChatAgent::Ollama(agent) => ChatStream::Ollama(
                agent
                    .stream_chat(message, history)
                    .max_turns(multi_turn)
                    .tool_concurrency(tool_concurrency)
                    .await,
            ),
            ChatAgent::Gemini(agent) => ChatStream::Gemini(
                agent
                    .stream_chat(message, history)
                    .max_turns(multi_turn)
                    .tool_concurrency(tool_concurrency)
                    .await,
            ),
            ChatAgent::OpenAI(agent) => ChatStream::OpenAI(
                agent
                    .stream_chat(message, history)
                    .max_turns(multi_turn)
                    .tool_concurrency(tool_concurrency)
                    .await,
            ),
            ChatAgent::Anthropic(agent) => ChatStream::Anthropic(
                agent
                    .stream_chat(message, history)
                    .max_turns(multi_turn)
                    .tool_concurrency(tool_concurrency)
                    .await,
            ),
            ChatAgent::Bedrock(agent) => ChatStream::Bedrock(
                agent
                    .stream_chat(message, history)
                    .max_turns(multi_turn)
                    .tool_concurrency(tool_concurrency)
                    .await,
            ),
        }
    }

    /// Non-streaming convenience: collects all text from a single-turn chat.
    /// Ignores tool calls - intended for lightweight sub-agent use (e.g. summarization).
    pub async fn chat(
        &self,
        message: impl Into<Message> + Send,
    ) -> Result<String, rig::agent::StreamingError> {
        let mut stream = self.stream_chat(message, vec![]).await;
        let mut full_text = String::new();
        let mut was_text = false;
        while let Some(result) = stream.next().await {
            match result {
                Ok(StreamItem::Text { text }) => {
                    if !was_text {
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
    directive: Vec<Directive>,
    tools: Vec<Box<dyn ToolDyn>>,
    thinking: bool,
) -> ChatAgent {
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
    match model.config {
        ProviderConfig::Ollama(config) => ChatAgent::Ollama(get_ollama_agent(
            config,
            model.model_name,
            resolved_directive,
            tools,
            thinking,
        )),
        ProviderConfig::Gemini(config) => ChatAgent::Gemini(get_gemini_agent(
            config,
            model.model_name,
            resolved_directive,
            tools,
            thinking,
        )),
        ProviderConfig::OpenAI(config) => ChatAgent::OpenAI(get_openai_agent(
            config,
            model.model_name,
            resolved_directive,
            tools,
            thinking,
        )),
        ProviderConfig::Anthropic(config) => ChatAgent::Anthropic(get_anthropic_agent(
            config,
            model.model_name,
            resolved_directive,
            tools,
            thinking,
        )),
        ProviderConfig::Bedrock(_config) => ChatAgent::Bedrock(get_bedrock_agent(
            model.model_name,
            resolved_directive,
            tools,
            thinking,
        )),
    }
}
