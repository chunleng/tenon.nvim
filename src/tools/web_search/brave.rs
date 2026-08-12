use rig::tool::ToolExecutionError;
use serde::Deserialize;

use super::{Freshness, SearchProvider};

use async_trait::async_trait;

#[derive(Deserialize)]
struct BraveSearchResponse {
    web: Option<WebResults>,
}

#[derive(Deserialize)]
struct WebResults {
    results: Vec<WebResult>,
}

#[derive(Deserialize)]
struct WebResult {
    title: String,
    url: String,
    description: Option<String>,
}

/// Maps `Freshness` to the Brave API's freshness format.
fn to_brave_freshness(freshness: Freshness) -> &'static str {
    match freshness {
        Freshness::D => "pd",
        Freshness::W => "pw",
        Freshness::M => "pm",
        Freshness::Y => "py",
    }
}

/// Brave Search web search API provider.
pub struct Brave {
    pub api_key: String,
}

#[async_trait]
impl SearchProvider for Brave {
    async fn search(
        &self,
        query: &str,
        freshness: Option<Freshness>,
        count: u8,
        region: Option<String>,
    ) -> Result<Vec<super::SearchResult>, ToolExecutionError> {
        let mut req = reqwest::Client::new()
            .get("https://api.search.brave.com/res/v1/web/search")
            .header("X-Subscription-Token", &self.api_key)
            .header("Accept", "application/json")
            .query(&[("q", query), ("count", &count.clamp(1, 20).to_string())]);

        if let Some(freshness) = freshness {
            req = req.query(&[("freshness", to_brave_freshness(freshness))]);
        }

        // Brave defaults country to US when unset; send ALL explicitly for
        // worldwide results when no region is requested.
        req = req.query(&[("country", region.as_deref().unwrap_or("ALL"))]);

        let mut attempt: u32 = 0;
        let resp = loop {
            let resp = req
                .try_clone()
                .ok_or_else(|| ToolExecutionError::other("Request not cloneable"))?
                .send()
                .await
                .map_err(|e| ToolExecutionError::other(format!("Request failed: {}", e)))?;

            let status = resp.status();
            if status.is_success() {
                break resp;
            }

            // Only retry on 5xx server errors or 429 throttle.
            let retryable = status.is_server_error() || status.as_u16() == 429;
            if !retryable || attempt >= super::MAX_RETRIES {
                break resp;
            }

            attempt += 1;
            tokio::time::sleep(super::retry_backoff(attempt)).await;
        };

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            return Err(ToolExecutionError::other(format!(
                "API {} → {}",
                status, text
            )));
        }

        let search_resp: BraveSearchResponse = resp
            .json()
            .await
            .map_err(|e| ToolExecutionError::other(format!("Bad response: {}", e)))?;

        Ok(search_resp
            .web
            .map(|w| w.results)
            .unwrap_or_default()
            .into_iter()
            .map(|result| super::SearchResult {
                name: result.title,
                url: result.url,
                snippet: result.description.unwrap_or_default(),
            })
            .collect())
    }
}
