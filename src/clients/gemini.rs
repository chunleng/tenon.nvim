use crate::clients::ApiKey;
use rig::{
    ProviderError,
    agent::{Agent, AgentBuilder},
    driver::{Exchange, Model, Opening, Transport},
    http_client::{DynHttpClient, HeaderValue},
    providers::gemini::{GeminiConfig, completion::GenerateContent},
    tool::DynamicTool,
    wire::{Encoded, WireFrame},
};
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct GeminiProviderConfig {
    pub base_url: String,
    pub api_key: ApiKey,
    pub authentication: GeminiAuthentication,
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GeminiAuthentication {
    QueryKey,
    Bearer,
}

#[derive(Clone)]
struct GeminiTransport {
    inner: DynHttpClient,
    authentication: GeminiAuthentication,
    api_key: String,
}

impl GeminiTransport {
    #[allow(clippy::result_large_err)]
    fn rewrite(&self, payload: &mut Encoded) -> Result<(), ProviderError> {
        if matches!(self.authentication, GeminiAuthentication::QueryKey) {
            return Ok(());
        }
        let mut url = reqwest::Url::parse(&payload.request.uri().to_string())
            .map_err(ProviderError::request)?;
        let query: Vec<(String, String)> = url
            .query_pairs()
            .filter(|(name, _)| name != "key")
            .map(|(name, value)| (name.into_owned(), value.into_owned()))
            .collect();
        url.set_query(None);
        if !query.is_empty() {
            url.query_pairs_mut().extend_pairs(query);
        }
        let mut authorization = HeaderValue::from_str(&format!("Bearer {}", self.api_key))
            .map_err(ProviderError::request)?;
        authorization.set_sensitive(true);
        payload
            .request
            .headers_mut()
            .insert("authorization", authorization);
        *payload.request.uri_mut() = url.as_str().parse().map_err(ProviderError::request)?;
        Ok(())
    }
}

impl Transport<GenerateContent> for GeminiTransport {
    fn send(&self, mut payload: Encoded, exchange: Exchange) -> Opening<WireFrame> {
        if let Err(error) = self.rewrite(&mut payload) {
            return Opening::failed(error);
        }
        <DynHttpClient as Transport<GenerateContent>>::send(&self.inner, payload, exchange)
    }
}

impl Default for GeminiProviderConfig {
    fn default() -> Self {
        Self {
            base_url: "https://generativelanguage.googleapis.com".to_string(),
            api_key: ApiKey::Env {
                env: "GEMINI_API_KEY".to_string(),
            },
            authentication: GeminiAuthentication::QueryKey,
        }
    }
}

pub fn get_gemini_agent(
    config: GeminiProviderConfig,
    model_name: String,
    preamble: Option<String>,
    tools: Vec<DynamicTool>,
    mut params: serde_json::Map<String, serde_json::Value>,
) -> Agent {
    let api_key = config.api_key.resolve().unwrap_or_else(|e| {
        crate::utils::GLOBAL_EXECUTION_HANDLER
            .notify_on_main_thread(e.to_string(), nvim_oxi::api::types::LogLevel::Error);
        String::new()
    });
    // Keep bearer credentials out of Rig's generated query string.
    let query_key = match config.authentication {
        GeminiAuthentication::QueryKey => api_key.clone(),
        GeminiAuthentication::Bearer => String::new(),
    };
    let model = GeminiConfig::new(query_key)
        .with_base_url(config.base_url)
        .connect(crate::utils::rig_http::shared())
        .completion(model_name);
    let model = Model::new(
        model.wire,
        GeminiTransport {
            inner: model.transport,
            authentication: config.authentication,
            api_key,
        },
    );
    let mut agent = AgentBuilder::new(model);
    if let Some(p) = preamble {
        agent = agent.preamble(&p);
    }
    if let Some(max_tokens) = params.remove("max_tokens").and_then(|v| v.as_u64()) {
        agent = agent.max_tokens(max_tokens);
    }
    if !params.is_empty() {
        agent = agent.additional_params(serde_json::Value::Object(params));
    }

    agent
        .dynamic_tools(tools)
        .add_hook(crate::clients::InvalidToolCallHook)
        .build()
}
