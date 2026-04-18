use rig::completion::ToolDefinition;
use rig::tool::{Tool, ToolError};
use serde::{Deserialize, Serialize};
use serde_json::json;

#[derive(Deserialize)]
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
    type Error = ToolError;
    type Args = WebSearchArgs;
    type Output = String;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: "web_search".to_string(),
            description: "Search web → JSON results. Each: name, url, snippet, date_published, date_last_crawled.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "query": {
                        "type": "string",
                        "description": "Search query"
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
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        let api_key = std::env::var("LANGSEARCH_API_KEY").map_err(|_| {
            ToolError::ToolCallError(Box::new(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "LANGSEARCH_API_KEY not set",
            )))
        })?;

        let count = args.count.unwrap_or(10).min(10).max(1);

        let mut body = json!({
            "query": args.query,
            "count": count,
            "summary": false,
        });

        if let Some(freshness) = &args.freshness {
            body["freshness"] = json!(freshness);
        }

        let client = reqwest::Client::new();
        let resp = client
            .post("https://api.langsearch.com/v1/web-search")
            .header("Authorization", format!("Bearer {}", api_key))
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await
            .map_err(|e| {
                ToolError::ToolCallError(Box::new(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    format!("Request failed: {}", e),
                )))
            })?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            return Err(ToolError::ToolCallError(Box::new(std::io::Error::new(
                std::io::ErrorKind::Other,
                format!("API {} → {}", status, text),
            ))));
        }

        let search_resp: LangSearchResponse = resp.json().await.map_err(|e| {
            ToolError::ToolCallError(Box::new(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("Bad response: {}", e),
            )))
        })?;

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

        Ok(serde_json::to_string(&results)
            .unwrap_or_else(|e| format!("{{\"error\": \"Serialize failed: {}\"}}", e)))
    }
}
