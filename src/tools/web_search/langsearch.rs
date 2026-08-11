use rand::Rng;
use rig::tool::ToolExecutionError;
use serde::Deserialize;
use serde_json::json;
use std::sync::LazyLock;
use std::time::{Duration, Instant};

use super::{Freshness, SearchProvider};

/// Minimum gap between the end of one search and the start of the next,
/// shared across all chat sessions.
const SEARCH_MIN_INTERVAL: Duration = Duration::from_secs(1);

/// Serializes search calls and stores when the most recent search finished.
///
/// The lock is held across each HTTP request so searches run one at a time;
/// the recorded completion time then enforces `SEARCH_MIN_INTERVAL` between the
/// end of one search and the start of the next. A tokio (async) mutex is
/// required because the guard is held across `.await` points.
static LAST_SEARCH_DONE: LazyLock<tokio::sync::Mutex<Instant>> =
    LazyLock::new(|| tokio::sync::Mutex::new(Instant::now() - SEARCH_MIN_INTERVAL));

/// How long to wait before starting a search so at least `SEARCH_MIN_INTERVAL`
/// has elapsed since the previous search completed.
fn throttle_wait(last_done: Instant) -> Duration {
    SEARCH_MIN_INTERVAL.saturating_sub(last_done.elapsed())
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
struct LangSearchResponse {
    data: SearchData,
}

#[derive(Deserialize)]
struct SearchData {
    #[serde(rename = "webPages")]
    web_pages: WebPages,
}

#[derive(Deserialize)]
struct WebPages {
    value: Vec<WebPageValue>,
}

#[derive(Deserialize)]
struct WebPageValue {
    name: String,
    url: String,
    snippet: String,
}

/// LangSearch web search API provider.
pub struct LangSearch;

impl Freshness {
    /// Maps the short code to the LangSearch API's freshness format.
    fn to_api_string(self) -> &'static str {
        match self {
            Freshness::D => "oneDay",
            Freshness::W => "oneWeek",
            Freshness::M => "oneMonth",
            Freshness::Y => "oneYear",
        }
    }
}

impl SearchProvider for LangSearch {
    async fn search(
        &self,
        query: &str,
        freshness: Option<Freshness>,
        count: u8,
        _region: Option<String>,
    ) -> Result<Vec<super::SearchResult>, ToolExecutionError> {
        let api_key = std::env::var("LANGSEARCH_API_KEY")
            .map_err(|_| ToolExecutionError::not_found("LANGSEARCH_API_KEY not set"))?;

        let mut body = json!({
            "query": query,
            "count": count,
            "summary": false,
        });

        if let Some(freshness) = freshness {
            body["freshness"] = json!(freshness.to_api_string());
        }

        // Serialize calls and enforce the completion-based interval: hold the
        // lock across the request so only one search runs at a time.
        let mut last_done = LAST_SEARCH_DONE.lock().await;
        tokio::time::sleep(throttle_wait(*last_done)).await;

        let client = reqwest::Client::new();
        let mut attempt: u32 = 0;
        let resp = loop {
            let resp = client
                .post("https://api.langsearch.com/v1/web-search")
                .header("Authorization", format!("Bearer {}", api_key))
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
            if !retryable || attempt >= MAX_RETRIES {
                break resp;
            }

            attempt += 1;
            tokio::time::sleep(retry_backoff(attempt)).await;
        };

        // Search finished — stamp the completion time and release the lock so
        // the next call can proceed (it waits out the remainder of the interval).
        *last_done = Instant::now();
        drop(last_done);

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            return Err(ToolExecutionError::other(format!(
                "API {} → {}",
                status, text
            )));
        }

        let search_resp: LangSearchResponse = resp
            .json()
            .await
            .map_err(|e| ToolExecutionError::other(format!("Bad response: {}", e)))?;

        Ok(search_resp
            .data
            .web_pages
            .value
            .into_iter()
            .map(|page| super::SearchResult {
                name: page.name,
                url: page.url,
                snippet: page.snippet,
            })
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_wait_when_interval_elapsed_since_completion() {
        let last = Instant::now() - SEARCH_MIN_INTERVAL * 2;
        assert_eq!(throttle_wait(last), Duration::ZERO);
    }

    #[test]
    fn waits_remaining_time_within_interval() {
        let last = Instant::now() - Duration::from_millis(400);
        let wait = throttle_wait(last);
        assert!(
            (wait.as_millis() as i64 - 600).abs() < 50,
            "expected ~600ms remaining after 400ms, got {wait:?}"
        );
    }

    #[test]
    fn waits_full_interval_right_after_completion() {
        let last = Instant::now();
        let wait = throttle_wait(last);
        assert!(
            wait >= SEARCH_MIN_INTERVAL - Duration::from_millis(50),
            "expected ~1s wait right after completion, got {wait:?}"
        );
    }

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
