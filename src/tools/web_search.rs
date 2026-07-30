use rand::Rng;
use rig::tool::{Tool, ToolContext, ToolExecutionError};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::sync::LazyLock;
use std::time::{Duration, Instant};

/// Minimum gap between the end of one web_search and the start of the next,
/// shared across all chat sessions.
const SEARCH_MIN_INTERVAL: Duration = Duration::from_secs(1);

/// Number of retry attempts after the initial request fails with a retryable
/// status (5xx or 429 throttle).
const MAX_RETRIES: u32 = 4;

/// Serializes web_search calls and stores when the most recent search finished.
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
    pub freshness: Option<String>,
    pub count: Option<u8>,
}

#[derive(Deserialize, Serialize, Clone)]
pub struct WebSearch;

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
    #[serde(rename = "datePublished")]
    date_published: Option<String>,
}

impl Tool for WebSearch {
    const NAME: &'static str = "web_search";
    type Error = ToolExecutionError;
    type Args = WebSearchArgs;
    type Output = String;

    fn description(&self) -> String {
        "Search web → YAML results. Each: name, url, snippet, date_published, date_last_crawled."
            .to_string()
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
                    "description": "Time filter. \"oneDay\"|\"oneWeek\"|\"oneMonth\"|\"oneYear\"|\"noLimit\" (default)"
                },
                "count": {
                    "type": "integer",
                    "description": "Results count. 1-10. Default: 10"
                }
            },
            "required": ["query"]
        })
    }

    async fn call(
        &self,
        _context: &mut ToolContext,
        args: Self::Args,
    ) -> Result<Self::Output, Self::Error> {
        let api_key = std::env::var("LANGSEARCH_API_KEY")
            .map_err(|_| ToolExecutionError::not_found("LANGSEARCH_API_KEY not set"))?;

        let count = args.count.unwrap_or(10).clamp(1, 10);

        let mut body = json!({
            "query": args.query,
            "count": count,
            "summary": false,
        });

        if let Some(freshness) = &args.freshness {
            body["freshness"] = json!(freshness);
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

        let results: Vec<serde_json::Value> = search_resp
            .data
            .web_pages
            .value
            .into_iter()
            .map(|page| {
                let mut obj = json!({
                    "name": page.name,
                    "url": page.url,
                    "snippet": page.snippet,
                });
                if let Some(dp) = page.date_published {
                    obj["date_published"] = json!(dp);
                }
                obj
            })
            .collect();

        Ok(crate::utils::format_yaml_block_scalars(
            &serde_yaml::to_string(&results)
                .unwrap_or_else(|e| format!("error: \"Serialize failed: {}\"", e)),
        ))
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
