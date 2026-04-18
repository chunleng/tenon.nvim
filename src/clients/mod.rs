use reqwest::header::{AUTHORIZATION, HeaderMap, HeaderValue};
use rig::{
    agent::Agent,
    client::{CompletionClient, Nothing, ProviderClient},
    message::Message,
    providers::{gemini, ollama, openai},
    streaming::StreamingChat,
    tool::ToolDyn,
};

#[allow(dead_code)]
#[derive(Debug, Clone)]
pub enum SupportedModels {
    Ollama {
        config: OllamaProviderConfig,
        model_name: String,
    },
    Gemini {
        config: GeminiProviderConfig,
        model_name: String,
    },
    OpenAI {
        config: OpenAIProviderConfig,
        model_name: String,
    },
    Bedrock {
        model_name: String,
    },
}

impl SupportedModels {
    pub fn display_name(&self) -> String {
        match self {
            SupportedModels::Ollama { model_name, .. } => format!("ollama: {}", model_name),
            SupportedModels::Gemini { model_name, .. } => format!("gemini: {}", model_name),
            SupportedModels::OpenAI { model_name, .. } => format!("openai: {}", model_name),
            SupportedModels::Bedrock { model_name } => format!("bedrock: {}", model_name),
        }
    }
}

pub enum ChatAgent {
    Ollama(Agent<ollama::CompletionModel>),
    Gemini(Agent<gemini::CompletionModel>),
    OpenAI(Agent<openai::CompletionModel>),
    Bedrock(Agent<rig_bedrock::completion::CompletionModel>),
}

pub enum ChatStream {
    Ollama(
        futures::stream::BoxStream<
            'static,
            Result<
                rig::agent::MultiTurnStreamItem<ollama::StreamingCompletionResponse>,
                rig::agent::StreamingError,
            >,
        >,
    ),
    Gemini(
        futures::stream::BoxStream<
            'static,
            Result<
                rig::agent::MultiTurnStreamItem<gemini::streaming::StreamingCompletionResponse>,
                rig::agent::StreamingError,
            >,
        >,
    ),
    OpenAI(
        futures::stream::BoxStream<
            'static,
            Result<
                rig::agent::MultiTurnStreamItem<openai::streaming::StreamingCompletionResponse>,
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
            ChatAgent::Bedrock(agent) => ChatStream::Bedrock(
                agent
                    .stream_chat(message, history)
                    .multi_turn(multi_turn)
                    .await,
            ),
        }
    }
}

pub fn get_agent(
    model: SupportedModels,
    preamble: Option<String>,
    tools: Vec<Box<dyn ToolDyn>>,
) -> ChatAgent {
    match model {
        SupportedModels::Ollama { config, model_name } => {
            ChatAgent::Ollama(get_ollama_agent(config, model_name, preamble, tools))
        }
        SupportedModels::Gemini { config, model_name } => {
            ChatAgent::Gemini(get_gemini_agent(config, model_name, preamble, tools))
        }
        SupportedModels::OpenAI { config, model_name } => {
            ChatAgent::OpenAI(get_openai_agent(config, model_name, preamble, tools))
        }
        SupportedModels::Bedrock { model_name } => {
            ChatAgent::Bedrock(get_bedrock_agent(model_name, preamble, tools))
        }
    }
}

#[derive(Debug, Clone)]
pub struct OllamaProviderConfig {
    pub base_url: String,
    pub bearer: Option<String>,
}

impl Default for OllamaProviderConfig {
    fn default() -> Self {
        Self {
            base_url: "http://127.0.0.1:11434".to_string(),
            bearer: std::env::var("OLLAMA_API_KEY").ok(),
        }
    }
}

fn get_ollama_agent(
    config: OllamaProviderConfig,
    model_name: String,
    preamble: Option<String>,
    tools: Vec<Box<dyn ToolDyn>>,
) -> Agent<ollama::CompletionModel> {
    let mut headers = HeaderMap::new();
    if let Some(bearer) = config.bearer {
        headers.insert(
            AUTHORIZATION,
            HeaderValue::from_str(&format!("Bearer {}", bearer)).unwrap(),
        );
    }
    let client = ollama::Client::builder()
        .base_url(config.base_url)
        .http_headers(headers)
        .api_key(Nothing)
        .build()
        .unwrap();
    let mut agent = client.agent(model_name);
    if let Some(p) = preamble {
        agent = agent.preamble(&p);
    }
    let agent = agent
        .additional_params(serde_json::json!({ "think": true }))
        .tools(tools)
        .build();

    agent
}

#[derive(Debug, Clone)]
pub struct GeminiProviderConfig {
    pub base_url: String,
    pub api_key: String,
}

impl Default for GeminiProviderConfig {
    fn default() -> Self {
        Self {
            base_url: "https://generativelanguage.googleapis.com".to_string(),
            api_key: std::env::var("GEMINI_API_KEY").unwrap_or_default(),
        }
    }
}

fn get_gemini_agent(
    config: GeminiProviderConfig,
    model_name: String,
    preamble: Option<String>,
    tools: Vec<Box<dyn ToolDyn>>,
) -> Agent<gemini::CompletionModel> {
    let client = gemini::Client::builder()
        .base_url(config.base_url)
        .api_key(config.api_key)
        .build()
        .unwrap();
    let mut agent = client.agent(model_name);
    if let Some(p) = preamble {
        agent = agent.preamble(&p);
    }
    let agent = agent.tools(tools).build();

    agent
}

#[derive(Debug, Clone)]
pub struct OpenAIProviderConfig {
    pub base_url: String,
    pub api_key: String,
}

impl Default for OpenAIProviderConfig {
    fn default() -> Self {
        Self {
            base_url: "https://api.openai.com/v1".to_string(),
            api_key: std::env::var("OPENAI_API_KEY").unwrap_or_default(),
        }
    }
}

fn get_openai_agent(
    config: OpenAIProviderConfig,
    model_name: String,
    preamble: Option<String>,
    tools: Vec<Box<dyn ToolDyn>>,
) -> Agent<openai::CompletionModel> {
    let client = openai::Client::builder()
        .base_url(config.base_url)
        .api_key(config.api_key)
        .build()
        .unwrap()
        .completions_api();
    let mut agent = client.agent(model_name.clone());
    if let Some(p) = preamble {
        agent = agent.preamble(&p);
    }
    // non-exhaustive for thinking model for now
    if ["gpt-5.4", "o3", "o1"].contains(&model_name.as_str()) {
        agent = agent.additional_params(serde_json::json!({
            "reasoning_effort": "high"
        }));
    }
    let agent = agent.tools(tools).build();

    agent
}

fn get_bedrock_agent(
    model_name: String,
    preamble: Option<String>,
    tools: Vec<Box<dyn ToolDyn>>,
) -> Agent<rig_bedrock::completion::CompletionModel> {
    // There's no config provider because bedrock is configured solely by env. Following are some
    // environment that you can override to provide the necessary configuration to bedrock (apart
    // from the standard env like AWS_REGION)
    // - AWS_ENDPOINT_URL_BEDROCK_RUNTIME
    // - AWS_BEARER_TOKEN_BEDROCK
    let client = rig_bedrock::client::Client::from_env();
    let mut agent = client.agent(model_name);
    if let Some(p) = preamble {
        agent = agent.preamble(&p);
    }
    let agent = agent.tools(tools).build();

    agent
}
