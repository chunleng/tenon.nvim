use crate::agent::worker::simple::SimpleTenonWorkerAgent;
use crate::get_application_config;
use crate::utils::path_from_str;
use base64::{Engine, engine::general_purpose::STANDARD};

use rig::message::{ImageMediaType, Message, MimeType, UserContent};
use rig::tool::{Tool, ToolContext, ToolExecutionError};
use serde::{Deserialize, Serialize};
use serde_json::json;

fn is_url(s: &str) -> bool {
    s.starts_with("http://") || s.starts_with("https://")
}

fn is_svg(bytes: &[u8]) -> bool {
    match std::str::from_utf8(bytes) {
        Ok(text) => text.to_ascii_lowercase().contains("<svg"),
        Err(_) => false,
    }
}

/// Rasterize SVG to PNG at 1024px longest side, preserving aspect ratio.
fn rasterize_svg(bytes: &[u8]) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    use resvg::tiny_skia;
    use resvg::usvg;

    let mut opt = usvg::Options::default();
    opt.fontdb_mut().load_system_fonts();
    let tree = usvg::Tree::from_data(bytes, &opt)?;

    let size = tree.size().to_int_size();
    let (svg_w, svg_h) = (size.width(), size.height());

    let longest = svg_w.max(svg_h).max(1) as f32;
    let scale = 1024.0 / longest;

    let target_w = ((svg_w as f32 * scale).round() as u32).max(1);
    let target_h = ((svg_h as f32 * scale).round() as u32).max(1);

    let mut pixmap = tiny_skia::Pixmap::new(target_w, target_h).ok_or("Failed to create pixmap")?;
    resvg::render(
        &tree,
        tiny_skia::Transform::from_scale(scale, scale),
        &mut pixmap.as_mut(),
    );

    Ok(pixmap.encode_png()?)
}

/// Rasterize SVG to PNG if input is SVG. Otherwise return bytes unchanged.
/// Falls back to original bytes on rasterization failure.
fn rasterize_if_svg(original_bytes: &[u8]) -> Vec<u8> {
    if is_svg(original_bytes) {
        match rasterize_svg(original_bytes) {
            Ok(png_bytes) => png_bytes,
            Err(_) => original_bytes.to_vec(),
        }
    } else {
        original_bytes.to_vec()
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AnalyzeImageArgs {
    pub image: String,
    pub prompt: String,
}

#[derive(Deserialize, Serialize, Clone)]
pub struct AnalyzeImage;

impl Tool for AnalyzeImage {
    const NAME: &'static str = "analyze_image";
    type Error = ToolExecutionError;
    type Args = AnalyzeImageArgs;
    type Output = String;

    fn description(&self) -> String {
        "Analyze image and answer questions about its content. Accepts local file path or URL. Use to identify objects, read text, describe scenes, answer visual queries, or extract info. Returns text answer based on prompt."
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
                    "description": "Question or instruction about the image. Scalar: 'How many people are in this image?' Structured: 'List all visible objects with their colors.' Compressed: 'Summarize this image in one sentence.'"
                }
            },
            "required": ["image", "prompt"]
        })
    }

    async fn call(
        &self,
        _context: &mut ToolContext,
        args: Self::Args,
    ) -> Result<Self::Output, Self::Error> {
        let image_content = if is_url(&args.image) {
            UserContent::image_url(&args.image, None, None)
        } else {
            let image_path = path_from_str(&args.image);
            let bytes = std::fs::read(&image_path).map_err(|e| {
                ToolExecutionError::other(format!("Failed to read image '{}': {}", args.image, e))
            })?;
            let processed = rasterize_if_svg(&bytes);
            let base64_data = STANDARD.encode(&processed);

            let mime = match image::guess_format(&processed) {
                Ok(image::ImageFormat::Png) => "image/png",
                Ok(image::ImageFormat::Jpeg) => "image/jpeg",
                Ok(image::ImageFormat::Gif) => "image/gif",
                Ok(image::ImageFormat::WebP) => "image/webp",
                Ok(image::ImageFormat::Bmp) => "image/bmp",
                _ => "image/png",
            };
            let media_type = ImageMediaType::from_mime_type(mime).unwrap_or(ImageMediaType::PNG);
            UserContent::image_base64(base64_data, Some(media_type), None)
        };

        let content = vec![UserContent::text(&args.prompt), image_content];

        let message = Message::User { content };

        let config = get_application_config();
        let worker = SimpleTenonWorkerAgent::new(
            config.tools.analyze_image.model.clone(),
            "Answer based on the image content. No preamble or hedge.",
            None,
        )
        .map_err(|e| ToolExecutionError::from_error(e))?;

        let response = worker.chat(message).await.map_err(|e| {
            ToolExecutionError::other(format!("Agent failed to analyze image: {}", e))
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
    fn test_svg_rasterization() {
        let svg = br#"<svg xmlns="http://www.w3.org/2000/svg" width="200" height="100">
            <rect width="200" height="100" fill="white"/>
            <rect x="10" y="10" width="180" height="80" fill="black"/>
        </svg>"#;

        let result = rasterize_if_svg(svg);

        let decoded = image::load_from_memory(&result);
        assert!(
            decoded.is_ok(),
            "SVG should be rasterized to decodable PNG, got error: {:?}",
            decoded.err()
        );

        // Output should not be the original SVG XML bytes
        assert_ne!(
            result.as_slice(),
            svg.as_slice(),
            "SVG should be rasterized, not passed through as raw XML"
        );
    }

    #[test]
    fn test_svg_aspect_ratio() {
        let svg = br#"<svg xmlns="http://www.w3.org/2000/svg" width="200" height="100" viewBox="0 0 200 100">
            <rect width="200" height="100" fill="blue"/>
        </svg>"#;

        let result = rasterize_if_svg(svg);
        let decoded = image::load_from_memory(&result).unwrap();
        let (w, h) = (decoded.width(), decoded.height());

        let ratio = w as f64 / h as f64;
        assert!(
            (ratio - 2.0).abs() < 0.1,
            "SVG with 2:1 viewBox should preserve aspect ratio, got {ratio:.3} ({w}x{h})"
        );
    }

    #[test]
    fn test_svg_fallback() {
        let invalid_svg = b"<svg><this is not valid svg content>";
        let result = rasterize_if_svg(invalid_svg);
        assert_eq!(
            result.as_slice(),
            invalid_svg.as_slice(),
            "Malformed SVG that fails rasterization should fall back to original bytes"
        );
    }
}
