use crate::agent::worker::simple::SimpleTenonWorkerAgent;
use crate::get_application_config;
use crate::utils::load_image_file;

use rig::message::{Message, UserContent};
use rig::tool::{ToolContext, ToolExecutionError};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::tools::{ToolCore, ToolCoreCall};

fn is_url(s: &str) -> bool {
    s.starts_with("http://") || s.starts_with("https://")
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AnalyzeImageArgs {
    pub image: String,
    pub prompt: String,
}

#[derive(Deserialize, Serialize, Clone)]
pub struct AnalyzeImage;

pub struct AnalyzeImageCall {
    args: AnalyzeImageArgs,
}

impl ToolCoreCall for AnalyzeImageCall {
    type Error = ToolExecutionError;
    type Output = String;

    async fn result(self, _context: &mut ToolContext) -> Result<Self::Output, Self::Error> {
        let args = self.args;
        let image_content = if is_url(&args.image) {
            UserContent::image_url(&args.image, None, None)
        } else {
            UserContent::Image(load_image_file(&args.image).map_err(ToolExecutionError::other)?)
        };

        let content = vec![UserContent::text(&args.prompt), image_content];

        let message = Message::User { content };

        let config = get_application_config();
        let worker = SimpleTenonWorkerAgent::new(
            config.tools.analyze_image.model.clone(),
            "Answer based on the image content. No preamble or hedge.",
            None,
        )
        .map_err(ToolExecutionError::from_error)?;

        let response = worker.chat(message).await.map_err(|e| {
            ToolExecutionError::other(format!("Agent failed to analyze image: {}", e))
        })?;

        Ok(response)
    }
}

impl ToolCore for AnalyzeImage {
    fn name(&self) -> String {
        "analyze_image".to_string()
    }
    type Error = ToolExecutionError;
    type Args = AnalyzeImageArgs;
    type Output = String;
    type Call = AnalyzeImageCall;

    fn description(&self) -> String {
        "Analyze image and answer questions about its content. Accepts local file path or URL. Returns text answer based on prompt."
            .to_string()
    }

    fn parameters(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "image": {
                    "type": "string",
                    "description": "Path or URL to image. Supports common formats (PNG, JPEG, GIF, WebP, BMP, SVG)."
                },
                "prompt": {
                    "type": "string",
                    "description": "Question or instruction about the image"
                }
            },
            "required": ["image", "prompt"]
        })
    }

    async fn init_call(
        &self,
        _context: &mut ToolContext,
        args: Self::Args,
    ) -> Result<Self::Call, Self::Error> {
        Ok(AnalyzeImageCall { args })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn test_is_url() {
        assert!(is_url("https://example.com/image.png"));
        assert!(is_url("http://example.com/image.jpg"));
        assert!(!is_url("/tmp/screenshot.png"));
        assert!(!is_url("./local/image.jpg"));
        assert!(!is_url("image.png"));
    }
}
