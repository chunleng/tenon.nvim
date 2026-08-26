use crate::clients::{
    ProviderConfig, SupportedModels, get_anthropic_agent, get_bedrock_agent, get_gemini_agent,
    get_ollama_agent, get_openai_completion_api_agent, get_openai_response_api_agent,
};
use crate::directive::Directive;
use rig::agent::{Agent, MultiTurnStreamItem, StreamingResult};
use rig::message::Message;
use rig::streaming::{StreamedAssistantContent, StreamedUserContent, StreamingChat};
use rig::tool::DynamicTool;

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
    CompletionCall {
        usage: rig::completion::Usage,
    },
    Other,
}

fn convert_stream_item(item: MultiTurnStreamItem) -> StreamItem {
    match item {
        MultiTurnStreamItem::StreamUserItem(StreamedUserContent::ToolResult {
            tool_result,
            internal_call_id,
        }) => StreamItem::ToolResult {
            tool_result,
            internal_call_id,
        },
        MultiTurnStreamItem::StreamAssistantItem(StreamedAssistantContent::ReasoningDelta {
            reasoning,
            ..
        }) => StreamItem::ReasoningDelta { reasoning },
        MultiTurnStreamItem::StreamAssistantItem(StreamedAssistantContent::Text(text_struct)) => {
            StreamItem::Text {
                text: text_struct.text,
            }
        }
        MultiTurnStreamItem::StreamAssistantItem(StreamedAssistantContent::ToolCall {
            tool_call,
            internal_call_id,
        }) => StreamItem::ToolCall {
            tool_call,
            internal_call_id,
        },
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
    pub async fn new(
        agent: &Agent,
        message: impl Into<Message> + Send,
        history: Vec<Message>,
        max_turns: usize,
    ) -> Self {
        let tool_concurrency = 10;
        let inner = agent
            .stream_chat(message, history)
            .max_turns(max_turns)
            .tool_concurrency(tool_concurrency)
            .await;
        ChatStream { inner }
    }

    pub async fn next(&mut self) -> Option<Result<StreamItem, rig::agent::StreamingError>> {
        use futures::stream::StreamExt;
        self.inner
            .next()
            .await
            .map(|result| result.map(convert_stream_item))
    }
}

pub fn get_agent(
    model: SupportedModels,
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
    let params = override_params.unwrap_or_else(|| model.default_parameters.clone());
    match model.config {
        ProviderConfig::Ollama(config) => {
            get_ollama_agent(config, model.model_name, resolved_directive, tools, params)
        }
        ProviderConfig::Gemini(config) => {
            get_gemini_agent(config, model.model_name, resolved_directive, tools, params)
        }
        ProviderConfig::OpenAICompletion(config) => get_openai_completion_api_agent(
            config,
            model.model_name,
            resolved_directive,
            tools,
            params,
        ),
        ProviderConfig::OpenAIResponse(config) => get_openai_response_api_agent(
            config,
            model.model_name,
            resolved_directive,
            tools,
            params,
        ),
        ProviderConfig::Anthropic(config) => {
            get_anthropic_agent(config, model.model_name, resolved_directive, tools, params)
        }
        ProviderConfig::Bedrock(_config) => {
            get_bedrock_agent(model.model_name, resolved_directive, tools, params)
        }
    }
}
