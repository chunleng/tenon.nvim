use crate::clients::get_agent;
use crate::directive::{Directive, DirectiveSource};
use crate::get_application_config;
use base64::{Engine, engine::general_purpose::STANDARD};
use rig::OneOrMany;
use rig::completion::ToolDefinition;
use rig::message::{ImageMediaType, Message, MimeType, UserContent};
use rig::tool::{Tool, ToolError};
use serde::{Deserialize, Serialize};
use serde_json::json;

fn is_url(s: &str) -> bool {
    s.starts_with("http://") || s.starts_with("https://")
}

fn image_mime_type(path: &str) -> &'static str {
    match path
        .rsplit('.')
        .next()
        .unwrap_or("")
        .to_lowercase()
        .as_str()
    {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "bmp" => "image/bmp",
        _ => "image/png",
    }
}

fn read_image_as_data_url(path: &str) -> Result<String, ToolError> {
    let bytes = std::fs::read(path).map_err(|e| {
        ToolError::ToolCallError(Box::new(std::io::Error::other(format!(
            "Failed to read image '{}': {}",
            path, e
        ))))
    })?;
    let encoded = STANDARD.encode(&bytes);
    let mime = image_mime_type(path);
    Ok(format!("data:{mime};base64,{encoded}"))
}

#[derive(Deserialize)]
pub struct AnalyzeImageArgs {
    pub image: String,
    pub prompt: String,
}

#[derive(Deserialize, Serialize, Clone)]
pub struct AnalyzeImage;

impl Tool for AnalyzeImage {
    const NAME: &'static str = "analyze_image";
    type Error = ToolError;
    type Args = AnalyzeImageArgs;
    type Output = String;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: "analyze_image".to_string(),
            description:
                "Analyze image and answer questions about its content. Accepts local file path or URL. Use to identify objects, read text, describe scenes, answer visual queries, or extract info. Returns text answer based on prompt."
                    .to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "image": {
                        "type": "string",
                        "description": "Path or URL to image. Supports common formats (PNG, JPEG, GIF, WebP, BMP)."
                    },
                    "prompt": {
                        "type": "string",
                        "description": "Question or instruction about the image. Scalar: 'How many people are in this image?' Structured: 'List all visible objects with their colors.' Compressed: 'Summarize this image in one sentence.'"
                    }
                },
                "required": ["image", "prompt"]
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        let image_content = if is_url(&args.image) {
            UserContent::image_url(&args.image, None, None)
        } else {
            let data_url = read_image_as_data_url(&args.image)?;
            let base64_data = data_url
                .split_once(',')
                .map(|(_, b64)| b64.to_string())
                .ok_or_else(|| {
                    ToolError::ToolCallError(Box::new(std::io::Error::other(
                        "Invalid data URL format",
                    )))
                })?;
            let media_type = ImageMediaType::from_mime_type(image_mime_type(&args.image))
                .unwrap_or(ImageMediaType::PNG);
            UserContent::image_base64(base64_data, Some(media_type), None)
        };

        let content = OneOrMany::many(vec![UserContent::text(&args.prompt), image_content])
            .map_err(|_| {
                ToolError::ToolCallError(Box::new(std::io::Error::other(
                    "Failed to build message content",
                )))
            })?;

        let message = Message::User { content };

        let config = get_application_config();
        let model = match &config.tools.analyze_image.model {
            Some(m) => m.clone(),
            None => {
                let agent_config = config.agents.get(&config.default_agent).ok_or_else(|| {
                    ToolError::ToolCallError(Box::new(std::io::Error::new(
                        std::io::ErrorKind::NotFound,
                        "No default agent",
                    )))
                })?;
                agent_config.model.clone()
            }
        };

        let directive = Directive {
            condition: None,
            source: DirectiveSource::Text {
                value: "Answer based on the image content. No preamble or hedge.".to_string(),
            },
        };

        let agent = get_agent(model, vec![directive], vec![], false);

        let response = agent.chat(message).await.map_err(|e| {
            ToolError::ToolCallError(Box::new(std::io::Error::other(format!(
                "Agent failed to analyze image: {}",
                e
            ))))
        })?;

        Ok(response)
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

    #[test]
    fn test_read_image_as_data_url() {
        let temp_path = std::env::temp_dir().join("tenon_test_image.png");
        std::fs::write(&temp_path, b"fake png content").unwrap();

        let result = read_image_as_data_url(temp_path.to_str().unwrap());
        assert!(result.is_ok(), "Should read image: {:?}", result.err());

        let data_url = result.unwrap();
        assert!(
            data_url.starts_with("data:image/png;base64,"),
            "Should produce PNG data URL, got: {data_url}"
        );
        assert!(
            data_url.len() > "data:image/png;base64,".len(),
            "Base64 part should be non-empty"
        );

        std::fs::remove_file(&temp_path).ok();
    }
}
