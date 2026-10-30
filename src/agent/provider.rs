use crate::clients::{
    ProviderKind, SelectedModel, get_anthropic_agent, get_bedrock_agent, get_gemini_agent,
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

pub fn get_agent(
    selected: &SelectedModel,
    directive: Vec<Directive>,
    tools: Vec<DynamicTool>,
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
    // Parameter state was fully resolved once at selection (provider defaults
    // overlaid by the definition's model-level defaults).
    let params = selected.parameters.clone();
    let definition = selected.definition();
    match definition.config.kind.clone() {
        ProviderKind::Ollama(config) => get_ollama_agent(
            config,
            definition.model_name.clone(),
            resolved_directive,
            tools,
            params,
        ),
        ProviderKind::Gemini(config) => get_gemini_agent(
            config,
            definition.model_name.clone(),
            resolved_directive,
            tools,
            params,
        ),
        ProviderKind::OpenAICompletion(config) => get_openai_completion_api_agent(
            config,
            definition.model_name.clone(),
            resolved_directive,
            tools,
            params,
        ),
        ProviderKind::OpenAIResponse(config) => get_openai_response_api_agent(
            config,
            definition.model_name.clone(),
            resolved_directive,
            tools,
            params,
        ),
        ProviderKind::Anthropic(config) => get_anthropic_agent(
            config,
            definition.model_name.clone(),
            resolved_directive,
            tools,
            params,
        ),
        ProviderKind::Bedrock(_config) => get_bedrock_agent(
            definition.model_name.clone(),
            resolved_directive,
            tools,
            params,
        ),
    }
}
