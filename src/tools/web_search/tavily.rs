use rig::tool::ToolExecutionError;
use serde::Deserialize;
use serde_json::json;

use super::{Freshness, SearchProvider};

use async_trait::async_trait;

#[derive(Deserialize)]
struct TavilySearchResponse {
    results: Vec<TavilyResult>,
}

#[derive(Deserialize)]
struct TavilyResult {
    title: String,
    url: String,
    content: String,
}

/// Maps `Freshness` to the Tavily API's `time_range` format.
fn to_tavily_time_range(freshness: Freshness) -> &'static str {
    match freshness {
        Freshness::D => "day",
        Freshness::W => "week",
        Freshness::M => "month",
        Freshness::Y => "year",
    }
}

/// Maps a 2-character ISO country code to the Tavily API's `country` format
/// (full lowercase country name). Returns `None` for unmapped codes.
fn to_tavily_country(code: &str) -> Option<&'static str> {
    let map = [
        ("us", "united states"),
        ("gb", "united kingdom"),
        ("ca", "canada"),
        ("au", "australia"),
        ("de", "germany"),
        ("fr", "france"),
        ("jp", "japan"),
        ("cn", "china"),
        ("in", "india"),
        ("br", "brazil"),
        ("it", "italy"),
        ("es", "spain"),
        ("nl", "netherlands"),
        ("se", "sweden"),
        ("no", "norway"),
        ("dk", "denmark"),
        ("fi", "finland"),
        ("be", "belgium"),
        ("ch", "switzerland"),
        ("at", "austria"),
        ("ie", "ireland"),
        ("pt", "portugal"),
        ("pl", "poland"),
        ("ru", "russia"),
        ("kr", "south korea"),
        ("kp", "north korea"),
        ("mx", "mexico"),
        ("ar", "argentina"),
        ("cl", "chile"),
        ("co", "colombia"),
        ("za", "south africa"),
        ("eg", "egypt"),
        ("ng", "nigeria"),
        ("ke", "kenya"),
        ("sa", "saudi arabia"),
        ("ae", "united arab emirates"),
        ("il", "israel"),
        ("tr", "turkey"),
        ("ir", "iran"),
        ("iq", "iraq"),
        ("th", "thailand"),
        ("id", "indonesia"),
        ("my", "malaysia"),
        ("sg", "singapore"),
        ("ph", "philippines"),
        ("vn", "vietnam"),
        ("nz", "new zealand"),
        ("ua", "ukraine"),
        ("cz", "czech republic"),
        ("gr", "greece"),
        ("hu", "hungary"),
        ("ro", "romania"),
        ("sk", "slovakia"),
        ("si", "slovenia"),
        ("hr", "croatia"),
        ("rs", "serbia"),
        ("bg", "bulgaria"),
        ("lt", "lithuania"),
        ("lv", "latvia"),
        ("ee", "estonia"),
        ("is", "iceland"),
        ("lu", "luxembourg"),
        ("mt", "malta"),
        ("cy", "cyprus"),
        ("tw", "taiwan"),
        ("hk", "hong kong"),
    ];

    map.iter()
        .find(|(c, _)| c.eq_ignore_ascii_case(code))
        .map(|(_, name)| *name)
}

/// Tavily Search web search API provider.
pub struct Tavily {
    pub api_key: String,
}

#[async_trait]
impl SearchProvider for Tavily {
    async fn search(
        &self,
        query: &str,
        freshness: Option<Freshness>,
        count: u8,
        region: Option<String>,
    ) -> Result<Vec<super::SearchResult>, ToolExecutionError> {
        let mut body = json!({
            "query": query,
            "max_results": count.clamp(1, 20),
        });

        if let Some(freshness) = freshness {
            body["time_range"] = json!(to_tavily_time_range(freshness));
        }

        if let Some(ref region) = region
            && let Some(country) = to_tavily_country(region)
        {
            body["country"] = json!(country);
        }

        let client = reqwest::Client::new();
        let mut attempt: u32 = 0;
        let resp = loop {
            let resp = client
                .post("https://api.tavily.com/search")
                .header("Authorization", format!("Bearer {}", self.api_key))
                .header("Content-Type", "application/json")
                .json(&body)
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

        let search_resp: TavilySearchResponse = resp
            .json()
            .await
            .map_err(|e| ToolExecutionError::other(format!("Bad response: {}", e)))?;

        Ok(search_resp
            .results
            .into_iter()
            .map(|result| super::SearchResult {
                name: result.title,
                url: result.url,
                snippet: result.content,
            })
            .collect())
    }
}
