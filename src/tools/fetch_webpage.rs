use crate::agent::worker::SimpleTenonWorkerAgent;
use crate::get_application_config;
use html_to_markdown_rs::{ConversionOptions, PreprocessingOptions, PreprocessingPreset};

use rig::tool::{Tool, ToolError};
use serde::{Deserialize, Serialize};
use serde_json::json;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FetchWebpageArgs {
    pub url: String,
    pub prompt: Option<String>,
}

#[derive(Deserialize, Serialize, Clone)]
pub struct FetchWebpage;

impl Tool for FetchWebpage {
    const NAME: &'static str = "fetch_webpage";
    type Error = ToolError;
    type Args = FetchWebpageArgs;
    type Output = String;

    fn description(&self) -> String {
        "Fetch webpage → readable text. w/ prompt (RECOMMENDED): answer from content. Else: full markdown"
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

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        let response = reqwest::get(&args.url).await.map_err(|e| {
            ToolError::ToolCallError(Box::new(std::io::Error::other(format!(
                "Fetch failed: '{}' → {}",
                args.url, e
            ))))
        })?;

        let content_type = response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok());

        let markdown = if is_pdf_content_type(content_type) {
            let bytes = response.bytes().await.map_err(|e| {
                ToolError::ToolCallError(Box::new(std::io::Error::other(format!(
                    "Read body failed: {}",
                    e
                ))))
            })?;
            process_pdf(bytes)
        } else {
            let html = response.text().await.map_err(|e| {
                ToolError::ToolCallError(Box::new(std::io::Error::other(format!(
                    "Read body failed: {}",
                    e
                ))))
            })?;
            process_html(&html)
        }?;

        match args.prompt {
            Some(prompt) => answer_with_prompt(&markdown, &prompt).await,
            None => Ok(markdown),
        }
    }
}

async fn answer_with_prompt(markdown: &str, prompt: &str) -> Result<String, ToolError> {
    let config = get_application_config();
    let worker = SimpleTenonWorkerAgent::new(
        config.tools.fetch_webpage.model.clone(),
        "Use only the webpage content. If the prompt cannot be answered from the content, say \"The page loaded successfully but does not contain the requested information.\" Do not infer or fabricate. Webpage content only. No preamble/hedge/commentary/source refs. Preserve format: code→code blocks, steps→numbered lists, comparisons→tables, items→bullets",
        true,
    )
    .map_err(|e| {
        ToolError::ToolCallError(Box::new(e))
    })?;

    let user_message = format!("{}\n\nWebpage content:\n\n{}", prompt, markdown);

    let response = worker.chat(user_message).await.map_err(|e| {
        ToolError::ToolCallError(Box::new(std::io::Error::other(format!(
            "Agent fail to run prompt: {}",
            e
        ))))
    })?;

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
fn process_html(html: &str) -> Result<String, ToolError> {
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
    .map_err(|e| {
        ToolError::ToolCallError(Box::new(std::io::Error::other(format!(
            "HTML→markdown failed: {}",
            e
        ))))
    })
}

/// Process PDF bytes into markdown.
fn process_pdf(bytes: bytes::Bytes) -> Result<String, ToolError> {
    let doc = unpdf::parse_bytes(&bytes).map_err(|e| {
        ToolError::ToolCallError(Box::new(std::io::Error::other(format!(
            "PDF parse failed: {}",
            e
        ))))
    })?;

    unpdf::render::to_markdown(&doc, &unpdf::render::RenderOptions::default()).map_err(|e| {
        ToolError::ToolCallError(Box::new(std::io::Error::other(format!(
            "PDF→markdown failed: {}",
            e
        ))))
    })
}
