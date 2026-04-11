use reqwest::header::{AUTHORIZATION, HeaderMap, HeaderValue};
use rig::{
    agent::Agent,
    client::{CompletionClient, Nothing},
    message::Message,
    providers::{gemini, ollama, openai},
    streaming::StreamingChat,
    tool::{Tool, ToolDyn},
};

#[allow(dead_code)]
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
}

pub enum ChatAgent {
    Ollama(Agent<ollama::CompletionModel>),
    Gemini(Agent<gemini::CompletionModel>),
    OpenAI(Agent<openai::CompletionModel>),
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
}

pub enum StreamItem {
    ToolResult {
        tool_result: rig::message::ToolResult,
        internal_call_id: String,
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
        }
    }
}

impl ChatAgent {
    pub async fn stream_chat(&self, message: String, history: Vec<Message>) -> ChatStream {
        let multi_turn = 3;
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
        }
    }
}

pub fn get_agent(
    model: SupportedModels,
    preamble: Option<String>,
    tools: Vec<impl Tool + 'static>,
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
    }
}

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
    tools: Vec<impl Tool + 'static>,
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
        .tools(
            tools
                .into_iter()
                .map(|t| Box::new(t) as Box<dyn ToolDyn>)
                .collect(),
        )
        .build();

    agent
}

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
    tools: Vec<impl Tool + 'static>,
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
    let agent = agent
        .tools(
            tools
                .into_iter()
                .map(|t| Box::new(t) as Box<dyn ToolDyn>)
                .collect(),
        )
        .build();

    agent
}

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
    tools: Vec<impl Tool + 'static>,
) -> Agent<openai::CompletionModel> {
    let client = openai::Client::builder()
        .base_url(config.base_url)
        .api_key(config.api_key)
        .build()
        .unwrap()
        .completions_api();
    let mut agent = client.agent(model_name);
    if let Some(p) = preamble {
        agent = agent.preamble(&p);
    }
    let agent = agent
        .tools(
            tools
                .into_iter()
                .map(|t| Box::new(t) as Box<dyn ToolDyn>)
                .collect(),
        )
        .build();

    agent
}
