use std::sync::atomic::Ordering;
use std::sync::{Arc, RwLock};

use crate::agent::engine::agentic::ContinueSignal;
use rig::message::ToolResultContent;
use rig::tool::{ToolContext, ToolExecutionError, ToolOutput};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::chat::{TenonImageLog, TenonLog, TenonLogData};
use crate::tools::{ToolCore, ToolCoreCall};
use crate::utils::load_image_file;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LoadImageArgs {
    pub filepath: String,
}

#[derive(Deserialize, Serialize, Clone)]
pub struct LoadImage;

pub struct LoadImageCall {
    filepath: String,
}

impl ToolCoreCall for LoadImageCall {
    type Error = ToolExecutionError;
    type Output = ToolOutput;

    async fn result(self, context: &mut ToolContext) -> Result<Self::Output, Self::Error> {
        let image = load_image_file(&self.filepath).map_err(ToolExecutionError::other)?;
        // End the current stream and start a new request: the log is swapped
        // to a user image log, so the image is delivered as a user message
        if let Some(signal) = context.get::<Arc<ContinueSignal>>() {
            signal.0.store(true, Ordering::Release);
        }
        Ok(ToolOutput::one(ToolResultContent::Image(image)))
    }
}

impl ToolCore for LoadImage {
    fn name(&self) -> String {
        "load_image".to_string()
    }
    type Error = ToolExecutionError;
    type Args = LoadImageArgs;
    type Output = ToolOutput;
    type Call = LoadImageCall;

    fn description(&self) -> String {
        "Load an image file into the chat so you can see it. Use this tool to view images (PNG, JPEG, GIF, WebP, BMP, SVG) instead of guessing their content. Returns the image itself."
            .to_string()
    }

    fn parameters(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "filepath": {
                    "type": "string",
                    "description": "Path to the image file. Supports common formats (PNG, JPEG, GIF, WebP, BMP, SVG)."
                }
            },
            "required": ["filepath"]
        })
    }

    async fn init_call(
        &self,
        _context: &mut ToolContext,
        args: Self::Args,
    ) -> Result<Self::Call, Self::Error> {
        Ok(LoadImageCall {
            filepath: args.filepath,
        })
    }

    /// The tool log becomes an image log; the engine restarts a new
    /// request (via `ContinueSignal`) so the image is delivered as a user
    /// message. The image is still returned as tool output as a fallback
    /// in case the log swap fails.
    async fn convert_log(&self, log: Arc<RwLock<TenonLog>>) -> Option<ToolOutput> {
        let mut log = log.write().ok()?;
        let tool_log = match &log.data {
            TenonLogData::Tool(tool_log) => tool_log.clone(),
            _ => return None,
        };
        log.data = TenonLogData::Image(TenonImageLog::Tool(tool_log));
        log.refresh();

        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::engine::agentic::ContinueSignal;
    use crate::chat::log::{TenonImageLog, TenonToolCall, TenonToolLog, TenonToolResult};
    use std::sync::atomic::{AtomicBool, Ordering};

    fn load_image_tool_log(filepath: &str, tool_result: TenonToolResult) -> TenonToolLog {
        TenonToolLog {
            tool_call: TenonToolCall {
                id: "call-1".to_string(),
                internal_call_id: "call-1".to_string(),
                item_id: None,
                name: "load_image".to_string(),
                args: serde_json::json!({"filepath": filepath}),
            },
            // convert_log only runs after set_tool_result, so an image log
            // always carries a result
            tool_result: Some(Ok(tool_result)),
            progress: vec![],
        }
    }

    #[tokio::test]
    async fn test_convert_log_swaps_tool_log_to_user_image_log() {
        let log = Arc::new(RwLock::new(TenonLog::new(TenonLogData::Tool(
            load_image_tool_log(
                "./img/diagram.png",
                TenonToolResult::Text(rig::agent::Text::default()),
            ),
        ))));

        // None keeps the original image output for the model
        assert!(LoadImage.convert_log(log.clone()).await.is_none());

        let log = log.read().unwrap();
        match log.data() {
            TenonLogData::Image(TenonImageLog::Tool(tool_log)) => {
                assert_eq!(
                    tool_log
                        .tool_call
                        .args
                        .get("filepath")
                        .and_then(|v| v.as_str()),
                    Some("./img/diagram.png")
                );
            }
            other => panic!("expected Image log, got {other:?}"),
        }
    }

    fn context_with_signal() -> (ToolContext, Arc<ContinueSignal>) {
        let signal = Arc::new(ContinueSignal(AtomicBool::new(false)));
        let mut context = ToolContext::new();
        context.insert(signal.clone());
        (context, signal)
    }

    #[tokio::test]
    async fn test_result_missing_file_is_error_and_does_not_signal() {
        let (mut context, signal) = context_with_signal();
        let call = LoadImageCall {
            filepath: "/nonexistent/image.png".to_string(),
        };
        assert!(call.result(&mut context).await.is_err());
        assert!(!signal.0.load(Ordering::SeqCst));
    }

    #[tokio::test]
    async fn test_result_success_sets_continue_signal() {
        let (mut context, signal) = context_with_signal();
        let dir = std::env::temp_dir().join("tenon_test_load_image_signal");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("pixel.png");
        std::fs::write(&path, utils_test_png()).unwrap();

        let call = LoadImageCall {
            filepath: path.to_string_lossy().to_string(),
        };
        let output = call.result(&mut context).await.unwrap();
        assert!(matches!(output.as_content(), [ToolResultContent::Image(_)]));
        assert!(signal.0.load(Ordering::SeqCst));
    }

    /// 1x1 red PNG
    fn utils_test_png() -> Vec<u8> {
        const PNG: &[u8] = &[
            0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48,
            0x44, 0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x02, 0x00, 0x00,
            0x00, 0x90, 0x77, 0x53, 0xDE, 0x00, 0x00, 0x00, 0x0C, 0x49, 0x44, 0x41, 0x54, 0x08,
            0xD7, 0x63, 0xF8, 0xCF, 0xC0, 0x00, 0x00, 0x03, 0x01, 0x01, 0x00, 0x18, 0xDD, 0x8D,
            0xB0, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4E, 0x44, 0xAE, 0x42, 0x60, 0x82,
        ];
        PNG.to_vec()
    }
}
