use crate::agent::worker::simple::SimpleTenonWorkerAgent;
use crate::get_application_config;
use html_to_markdown_rs::{ConversionOptions, PreprocessingOptions, PreprocessingPreset};
use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};
use std::time::{Duration, Instant};

use rig::tool::{ToolContext, ToolExecutionError};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::tools::{ToolCore, ToolCoreCall};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FetchWebpageArgs {
    pub url: String,
    pub prompt: Option<String>,
}

#[derive(Deserialize, Serialize, Clone)]
pub struct FetchWebpage;

pub struct FetchWebpageCall {
    args: FetchWebpageArgs,
}

impl ToolCoreCall for FetchWebpageCall {
    type Error = ToolExecutionError;
    type Output = String;

    async fn result(self, _context: &mut ToolContext) -> Result<Self::Output, Self::Error> {
        let args = self.args;
        let domain = reqwest::Url::parse(&args.url)
            .ok()
            .and_then(|u| u.host_str().map(str::to_string))
            .ok_or_else(|| ToolExecutionError::other(format!("Invalid URL: '{}'", args.url)))?;

        let wait = {
            let mut limiter = DOMAIN_RATE_LIMITER
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            limiter.wait_duration(&domain, Instant::now())
        };
        if !wait.is_zero() {
            tokio::time::sleep(wait).await;
        }

        let client = reqwest::Client::builder()
            .user_agent(format!(
                "Tenon/{} (+https://github.com/chunleng/tenon.nvim)",
                env!("CARGO_PKG_VERSION")
            ))
            .cookie_store(true)
            .build()
            .map_err(|e| ToolExecutionError::other(format!("Client build failed: {}", e)))?;

        let response = client.get(&args.url).send().await.map_err(|e| {
            ToolExecutionError::other(format!("Fetch failed: '{}' → {}", args.url, e))
        })?;

        let status = response.status();

        if !status.is_success() {
            return Err(ToolExecutionError::other(format!(
                "Fetch failed: '{}' → {}",
                args.url,
                status.as_u16(),
            )));
        }

        let status = status.as_u16();

        let content_type = response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok());

        let markdown = if is_pdf_content_type(content_type) {
            let bytes = response
                .bytes()
                .await
                .map_err(|e| ToolExecutionError::other(format!("Read body failed: {}", e)))?;
            process_pdf(bytes)
        } else {
            let html = response
                .text()
                .await
                .map_err(|e| ToolExecutionError::other(format!("Read body failed: {}", e)))?;
            process_html(&html)
        }?;

        let body = match args.prompt {
            Some(prompt) => answer_with_prompt(&markdown, &prompt).await?,
            None => markdown,
        };

        let output = crate::utils::format_yaml_block_scalars(
            &serde_yaml::to_string(&json!({
                "status": status,
                "response": body,
            }))
            .map_err(|e| ToolExecutionError::other(format!("Serialize output failed: {}", e)))?,
        );

        Ok(output)
    }
}

impl ToolCore for FetchWebpage {
    const NAME: &'static str = "fetch_webpage";
    type Error = ToolExecutionError;
    type Args = FetchWebpageArgs;
    type Output = String;
    type Call = FetchWebpageCall;

    fn description(&self) -> String {
        "Fetch webpage → readable text. Returns status code and response. With prompt (RECOMMENDED): answer from content. Else: full markdown"
            .to_string()
    }

    fn parameters(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "url": {
                    "type": "string",
                    "description": "URL"
                },
                "prompt": {
                    "type": "string",
                    "description": "What to extract/answer. Returns answer only. Scalar: fact/yes-no. Structured: table/steps/kvpairs. Compressed: summary/takeaways/translation. Filtered: partial document"
                }
            },
            "required": ["url"]
        })
    }

    async fn init_call(
        &self,
        _context: &mut ToolContext,
        args: Self::Args,
    ) -> Result<Self::Call, Self::Error> {
        Ok(FetchWebpageCall { args })
    }
}

/// Per-domain request backoff: 1st request no wait, 2nd 2s, 3rd 6s, 4th+ 10s.
/// 60s without a request to the domain resets the counter.
const DOMAIN_IDLE_RESET: Duration = Duration::from_secs(60);

static DOMAIN_RATE_LIMITER: LazyLock<Mutex<DomainRateLimiter>> =
    LazyLock::new(|| Mutex::new(DomainRateLimiter::default()));

#[derive(Default)]
struct DomainState {
    request_count: u32,
    last_request: Option<Instant>,
}

#[derive(Default)]
struct DomainRateLimiter {
    domains: HashMap<String, DomainState>,
}

impl DomainRateLimiter {
    /// Returns how long to wait before the next request to `domain`,
    /// and records the request at `now`.
    fn wait_duration(&mut self, domain: &str, now: Instant) -> Duration {
        let state = self.domains.entry(domain.to_string()).or_default();
        if let Some(last) = state.last_request
            && now.duration_since(last) >= DOMAIN_IDLE_RESET
        {
            state.request_count = 0;
        }
        let wait = match state.request_count {
            0 => Duration::ZERO,
            1 => Duration::from_secs(2),
            2 => Duration::from_secs(6),
            _ => Duration::from_secs(10),
        };
        state.request_count += 1;
        state.last_request = Some(now);
        wait
    }
}

async fn answer_with_prompt(markdown: &str, prompt: &str) -> Result<String, ToolExecutionError> {
    let config = get_application_config();
    let worker = SimpleTenonWorkerAgent::new(
        config.tools.fetch_webpage.model.clone(),
        "Use only the webpage content. If the prompt cannot be answered from the content, say \"The page loaded successfully but does not contain the requested information.\" Do not infer or fabricate. Webpage content only. No preamble/hedge/commentary/source refs. Preserve format: code→code blocks, steps→numbered lists, comparisons→tables, items→bullets",
        None,
    )
    .map_err(|e| {
        ToolExecutionError::from_error(e)
    })?;

    let user_message = format!("{}\n\nWebpage content:\n\n{}", prompt, markdown);

    let response = worker
        .chat(user_message)
        .await
        .map_err(|e| ToolExecutionError::other(format!("Agent fail to run prompt: {}", e)))?;

    Ok(response)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn get_test_data_dir() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("data/test_fixture")
    }

    #[test]
    fn test_is_pdf_content_type() {
        // PDF Content-Type should be detected
        assert!(is_pdf_content_type(Some("application/pdf")));
        assert!(is_pdf_content_type(Some("application/pdf; charset=utf-8")));
        assert!(is_pdf_content_type(Some("APPLICATION/PDF"))); // case-insensitive

        // Non-PDF Content-Type should not be detected as PDF
        assert!(!is_pdf_content_type(Some("text/html")));
        assert!(!is_pdf_content_type(Some("application/json")));
        assert!(!is_pdf_content_type(None));
    }

    #[test]
    fn test_process_html() {
        let html_path = get_test_data_dir().join("test_page.html");
        let html = std::fs::read_to_string(&html_path).expect("Failed to read test HTML file");

        let result = process_html(&html).unwrap();

        // Should preserve main content
        assert!(result.contains("Main Heading"));
        assert!(result.contains("This is a paragraph"));

        // Navigation should be removed by preprocessing
        assert!(!result.contains("/about"));
    }

    #[test]
    fn test_rate_limiter_backoff_schedule() {
        let mut limiter = DomainRateLimiter::default();
        let start = Instant::now();

        // 1st request: no wait
        assert_eq!(limiter.wait_duration("example.com", start), Duration::ZERO);
        // 2nd request: 2s
        assert_eq!(
            limiter.wait_duration("example.com", start + Duration::from_secs(1)),
            Duration::from_secs(2)
        );
        // 3rd request: 6s
        assert_eq!(
            limiter.wait_duration("example.com", start + Duration::from_secs(2)),
            Duration::from_secs(6)
        );
        // 4th and subsequent requests: 10s each
        assert_eq!(
            limiter.wait_duration("example.com", start + Duration::from_secs(3)),
            Duration::from_secs(10)
        );
        assert_eq!(
            limiter.wait_duration("example.com", start + Duration::from_secs(4)),
            Duration::from_secs(10)
        );
    }

    #[test]
    fn test_rate_limiter_resets_after_idle() {
        let mut limiter = DomainRateLimiter::default();
        let start = Instant::now();

        assert_eq!(limiter.wait_duration("example.com", start), Duration::ZERO);
        assert_eq!(
            limiter.wait_duration("example.com", start + Duration::from_secs(1)),
            Duration::from_secs(2)
        );

        // 60s of no requests to the domain resets the backoff
        let after_idle = start + Duration::from_secs(1) + Duration::from_secs(60);
        assert_eq!(
            limiter.wait_duration("example.com", after_idle),
            Duration::ZERO
        );
        // Counter restarts from the beginning
        assert_eq!(
            limiter.wait_duration("example.com", after_idle + Duration::from_secs(1)),
            Duration::from_secs(2)
        );
    }

    #[test]
    fn test_rate_limiter_per_domain() {
        let mut limiter = DomainRateLimiter::default();
        let start = Instant::now();

        assert_eq!(limiter.wait_duration("a.com", start), Duration::ZERO);
        // Requests to another domain are unaffected
        assert_eq!(limiter.wait_duration("b.com", start), Duration::ZERO);
        // a.com is still on its 2nd request
        assert_eq!(
            limiter.wait_duration("a.com", start + Duration::from_secs(1)),
            Duration::from_secs(2)
        );
    }

    #[test]
    fn test_process_pdf() {
        use bytes::Bytes;

        let pdf_path = get_test_data_dir().join("test_doc.pdf");
        let pdf_bytes =
            Bytes::from(std::fs::read(&pdf_path).expect("Failed to read test PDF file"));

        let result = process_pdf(pdf_bytes).expect("PDF processing should succeed");

        // Should extract text content from PDF
        assert!(
            result.contains("Hello World"),
            "PDF should contain 'Hello World', got: {result}"
        );
    }
}

/// Check if Content-Type header indicates a PDF document.
fn is_pdf_content_type(content_type: Option<&str>) -> bool {
    content_type
        .map(|ct| ct.to_lowercase().contains("application/pdf"))
        .unwrap_or(false)
}

/// Process HTML content into markdown.
fn process_html(html: &str) -> Result<String, ToolExecutionError> {
    html_to_markdown_rs::convert(
        html,
        Some(ConversionOptions {
            preprocessing: PreprocessingOptions {
                enabled: true,
                preset: PreprocessingPreset::Aggressive,
                remove_navigation: true,
                remove_forms: true,
            },
            ..Default::default()
        }),
    )
    .map_err(|e| ToolExecutionError::other(format!("HTML→markdown failed: {}", e)))?
    .content
    .ok_or_else(|| ToolExecutionError::other("HTML→markdown produced no content"))
}

/// Process PDF bytes into markdown.
fn process_pdf(bytes: bytes::Bytes) -> Result<String, ToolExecutionError> {
    let doc = unpdf::parse_bytes(&bytes)
        .map_err(|e| ToolExecutionError::other(format!("PDF parse failed: {}", e)))?;

    unpdf::render::to_markdown(&doc, &unpdf::render::RenderOptions::default())
        .map_err(|e| ToolExecutionError::other(format!("PDF→markdown failed: {}", e)))
}
