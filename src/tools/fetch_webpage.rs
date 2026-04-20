use html_to_markdown_rs::{ConversionOptions, PreprocessingOptions, PreprocessingPreset};
use rig::completion::ToolDefinition;
use rig::tool::{Tool, ToolError};
use serde::{Deserialize, Serialize};
use serde_json::json;

#[derive(Deserialize)]
pub struct FetchWebpageArgs {
    pub url: String,
}

#[derive(Deserialize, Serialize, Clone)]
pub struct FetchWebpage;

impl Tool for FetchWebpage {
    const NAME: &'static str = "fetch_webpage";
    type Error = ToolError;
    type Args = FetchWebpageArgs;
    type Output = String;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: "fetch_webpage".to_string(),
            description: "Fetch webpage → readable text for LLM".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "url": {
                        "type": "string",
                        "description": "URL to fetch"
                    }
                },
                "required": ["url"]
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        let html = reqwest::get(&args.url)
            .await
            .map_err(|e| {
                ToolError::ToolCallError(Box::new(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    format!("Failed to fetch URL '{}': {}", args.url, e),
                )))
            })?
            .text()
            .await
            .map_err(|e| {
                ToolError::ToolCallError(Box::new(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    format!("Failed to read response body: {}", e),
                )))
            })?;

        let markdown = html_to_markdown_rs::convert(
            &html,
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
            ToolError::ToolCallError(Box::new(std::io::Error::new(
                std::io::ErrorKind::Other,
                format!("Failed to convert HTML to markdown: {}", e),
            )))
        })?;

        Ok(markdown)
    }
}
