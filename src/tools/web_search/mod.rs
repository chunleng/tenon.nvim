mod brave;
mod langsearch;
mod tavily;

use crate::tools::{ToolCore, ToolCoreCall};
use async_trait::async_trait;
pub use brave::Brave;
pub use langsearch::LangSearch;
use rand::Rng;
use rig::tool::{ToolContext, ToolExecutionError};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::sync::Arc;
use std::time::Duration;
pub use tavily::Tavily;

/// A single web search result returned by a search provider.
#[derive(Serialize, Clone)]
pub struct SearchResult {
    pub name: String,
    pub url: String,
    pub snippet: String,
}

/// Time filter for web search results.
#[derive(Deserialize, Serialize, Clone, Copy)]
pub enum Freshness {
    #[serde(rename = "d")]
    D,
    #[serde(rename = "w")]
    W,
    #[serde(rename = "m")]
    M,
    #[serde(rename = "y")]
    Y,
}

/// Provider-agnostic interface for web search backends.
#[async_trait]
pub trait SearchProvider: Send + Sync {
    async fn search(
        &self,
        query: &str,
        freshness: Option<Freshness>,
        count: u8,
        region: Option<String>,
    ) -> Result<Vec<SearchResult>, ToolExecutionError>;
}

/// Number of retry attempts after the initial request fails with a retryable
/// status (5xx or 429 throttle).
const MAX_RETRIES: u32 = 4;

/// Random backoff before `retry`-th attempt (1-indexed): a random duration
/// between `retry` and `retry * 3` seconds.
fn retry_backoff(retry: u32) -> Duration {
    let mut rng = rand::rng();
    let secs = rng.random_range(retry..=(retry * 3));
    Duration::from_secs(secs as u64)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WebSearchArgs {
    pub query: String,
    pub freshness: Option<Freshness>,
    pub count: Option<u8>,
    pub region: Option<String>,
}

pub struct WebSearch {
    pub provider: Arc<dyn SearchProvider>,
}

pub struct WebSearchCall {
    args: WebSearchArgs,
    provider: Arc<dyn SearchProvider>,
}

impl ToolCoreCall for WebSearchCall {
    type Error = ToolExecutionError;
    type Output = String;

    async fn result(self, _context: &mut ToolContext) -> Result<Self::Output, Self::Error> {
        let count = self.args.count.unwrap_or(5);

        let results = self
            .provider
            .search(
                &self.args.query,
                self.args.freshness,
                count,
                self.args.region,
            )
            .await?;

        Ok(crate::utils::format_yaml_block_scalars(
            &serde_yaml::to_string(&results)
                .unwrap_or_else(|e| format!("error: \"Serialize failed: {}\"", e)),
        ))
    }
}

impl ToolCore for WebSearch {
    fn name(&self) -> String {
        "web_search".to_string()
    }
    type Error = ToolExecutionError;
    type Args = WebSearchArgs;
    type Output = String;
    type Call = WebSearchCall;

    fn description(&self) -> String {
        "Search web → YAML results. Each: name, url, snippet".to_string()
    }

    fn parameters(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "description": "Search query. No year/date unless user specified."
                },
                "freshness": {
                    "type": "string",
                    "enum": ["d", "w", "m", "y"],
                    "description": "Time filter. Omit for no limit."
                },
                "count": {
                    "type": "integer",
                    "description": "Results count",
                    "default": 5
                },
                "region": {
                    "type": "string",
                    "description": "2-character country code. Set to match the query's locale. Omit only for global queries"
                }
            },
            "required": ["query"]
        })
    }

    async fn init_call(
        &self,
        _context: &mut ToolContext,
        args: Self::Args,
    ) -> Result<Self::Call, Self::Error> {
        Ok(WebSearchCall {
            args,
            provider: Arc::clone(&self.provider),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retry_backoff_within_bounds() {
        for retry in 1..=MAX_RETRIES {
            let min = Duration::from_secs(retry as u64);
            let max = Duration::from_secs((retry * 3) as u64);
            for _ in 0..100 {
                let backoff = retry_backoff(retry);
                assert!(
                    backoff >= min && backoff <= max,
                    "retry {retry}: backoff {backoff:?} not within [{min:?}, {max:?}]"
                );
            }
        }
    }
}
