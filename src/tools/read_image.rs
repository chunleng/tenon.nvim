use std::sync::atomic::Ordering;
use std::sync::{Arc, RwLock};

use crate::agent::engine::agentic::ContinueSignal;
use crate::agent::worker::simple::SimpleTenonWorkerAgent;
use crate::clients::SelectedModel;
use rig::message::{Image, Message, ToolResultContent, UserContent};
use rig::tool::{ToolContext, ToolExecutionError, ToolOutput};
use serde::Deserialize;
use serde_json::json;

use crate::chat::{TenonImageLog, TenonLog, TenonLogData};
use crate::tools::{ToolCore, ToolCoreCall};
use crate::utils::{is_url, load_image_file, load_image_url};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReadImageArgs {
    pub filepath: String,
    pub prompt: Option<String>,
}

/// The vision model is resolved at construction:
/// - `Vision` carries the session model when it has vision
/// - `NonVision` carries the configured fallback model
#[derive(Clone)]
pub enum ReadImage {
    /// Session model has vision: allow image loading or subagent
    Vision { model: SelectedModel },
    /// Session model lacks vision: only subagent with `fallback_model`
    NonVision { fallback_model: SelectedModel },
}

impl ReadImage {
    /// Vision model for prompt mode.
    fn worker_model(&self) -> SelectedModel {
        match self {
            ReadImage::Vision { model } => model.clone(),
            ReadImage::NonVision { fallback_model } => fallback_model.clone(),
        }
    }
}

pub struct ReadImageCall {
    filepath: String,
    prompt: Option<String>,
    /// Vision model that answers in prompt mode.
    worker_model: SelectedModel,
}

impl ReadImageCall {
    /// The image to deliver. URLs are fetched and base64-encoded because some providers only accept
    /// base64 images; local paths are read from disk.
    async fn image(&self) -> Result<Image, ToolExecutionError> {
        if is_url(&self.filepath) {
            return load_image_url(&self.filepath)
                .await
                .map_err(ToolExecutionError::other);
        }
        load_image_file(&self.filepath).map_err(ToolExecutionError::other)
    }

    /// Delegates to a vision worker model: only its text answer reaches the
    /// caller's context, the image itself never does.
    async fn analyze(&self, prompt: &str) -> Result<ToolOutput, ToolExecutionError> {
        let message = Message::User {
            content: vec![
                UserContent::text(prompt),
                UserContent::Image(self.image().await?),
            ],
        };

        let worker = SimpleTenonWorkerAgent::new(
            Some(self.worker_model.clone()),
            "Answer based on the image content. No preamble or hedge.",
        )
        .map_err(ToolExecutionError::from_error)?;

        let response = worker.chat(message).await.map_err(|e| {
            ToolExecutionError::other(format!("Agent failed to analyze image: {}", e))
        })?;

        Ok(ToolOutput::text(response))
    }
}

impl ToolCoreCall for ReadImageCall {
    type Error = ToolExecutionError;
    type Output = ToolOutput;

    async fn result(self, context: &mut ToolContext) -> Result<Self::Output, Self::Error> {
        if let Some(prompt) = &self.prompt {
            return self.analyze(prompt).await;
        }

        let image = self.image().await?;
        // End the current stream and start a new request: the log is swapped
        // to a user image log, so the image is delivered as a user message
        if let Some(signal) = context.scope::<ContinueSignal>() {
            signal.0.store(true, Ordering::Release);
        }
        Ok(ToolOutput::one(ToolResultContent::Image(image)))
    }
}

impl ToolCore for ReadImage {
    fn name(&self) -> String {
        "read_image".to_string()
    }
    type Error = ToolExecutionError;
    type Args = ReadImageArgs;
    type Output = ToolOutput;
    type Call = ReadImageCall;

    fn description(&self) -> String {
        let mut description =
            "Read an image. Accepts a local file path or an http(s) URL. ".to_string();
        match self {
            ReadImage::Vision { .. } => description.push_str(
                "Two modes, chosen by whether you pass `prompt`:\n\
                 - Omit `prompt`: the image is loaded into your own context. You see it fully, \
                 which is best when you need to reason about the whole image.\n\
                 - Pass `prompt`: a separate vision model answers your question about the image, \
                 and only its text answer enters your context. You never see the image yourself.",
            ),
            ReadImage::NonVision { .. } => {
                description.push_str("A subagent answers your question about the image.");
            }
        }
        description
    }

    fn parameters(&self) -> serde_json::Value {
        let vision = matches!(self, ReadImage::Vision { .. });
        // Without vision, loading the image into the caller's context is
        // useless, so a prompt is the only way to use this tool
        let required = if vision {
            json!(["filepath"])
        } else {
            json!(["filepath", "prompt"])
        };
        // `default: null` marks prompt as omittable, which only holds for
        // vision models
        let mut prompt = json!({
            "type": "string",
            "description": "Question or instruction about the image."
        });
        if vision {
            prompt["default"] = serde_json::Value::Null;
        }
        json!({
            "type": "object",
            "properties": {
                "filepath": {
                    "type": "string",
                    "description": "Path or URL to the image. Supports common formats (PNG, JPEG, GIF, WebP, BMP, SVG)"
                },
                "prompt": prompt
            },
            "required": required
        })
    }

    fn properties(&self, _args: &ReadImageArgs) -> crate::chat::log::LogProperties {
        crate::chat::log::LogProperties {
            idempotent: true,
            important: false,
        }
    }

    async fn init_call(
        &self,
        _context: &mut ToolContext,
        args: Self::Args,
    ) -> Result<Self::Call, Self::Error> {
        if !matches!(self, ReadImage::Vision { .. }) && args.prompt.is_none() {
            // Mimic rig error when deserialize fails
            return Err(ToolExecutionError::invalid_args(
                "Failed to deserialize args: missing field `prompt`",
            ));
        }
        let worker_model = self.worker_model();

        Ok(ReadImageCall {
            filepath: args.filepath,
            prompt: args.prompt,
            worker_model,
        })
    }

    async fn convert_log(&self, log: Arc<RwLock<TenonLog>>) -> Option<ToolOutput> {
        let mut log = log.write().ok()?;
        let tool_log = match &log.data {
            TenonLogData::Tool(tool_log) => tool_log.clone(),
            _ => return None,
        };
        // The tool log becomes an image log when it is called with no prompt
        if tool_log
            .tool_call
            .args
            .get("prompt")
            .is_some_and(|v| !v.is_null())
        {
            return None;
        }
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
    use crate::clients::{OllamaProviderConfig, ProviderConfig, ProviderKind, SupportedModels};
    use std::collections::HashMap;

    use rig::message::DocumentSourceKind;
    use std::sync::atomic::{AtomicBool, Ordering};

    fn test_model(vision: bool) -> SelectedModel {
        SelectedModel::select(
            "test",
            &SupportedModels {
                connector_name: "test".to_string(),
                config: ProviderConfig {
                    kind: ProviderKind::Ollama(OllamaProviderConfig::default()),
                    parameters: HashMap::new(),
                },
                model_name: "test".to_string(),
                parameters: serde_json::Map::new(),
                vision,
            },
        )
    }

    fn vision_tool() -> ReadImage {
        ReadImage::Vision {
            model: test_model(true),
        }
    }

    fn read_image_tool_log(args: serde_json::Value, tool_result: TenonToolResult) -> TenonToolLog {
        TenonToolLog {
            tool_call: TenonToolCall {
                id: "call-1".to_string(),
                internal_call_id: "call-1".to_string(),
                item_id: None,
                name: "read_image".to_string(),
                args,
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
            read_image_tool_log(
                serde_json::json!({"filepath": "./img/diagram.png"}),
                TenonToolResult::Text(rig::agent::Text::default()),
            ),
        ))));

        // None keeps the original image output for the model
        assert!(vision_tool().convert_log(log.clone()).await.is_none());

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

    #[tokio::test]
    async fn test_convert_log_keeps_tool_log_in_prompt_mode() {
        let log = Arc::new(RwLock::new(TenonLog::new(TenonLogData::Tool(
            read_image_tool_log(
                serde_json::json!({"filepath": "./img/diagram.png", "prompt": "What color?"}),
                TenonToolResult::Text(rig::agent::Text::new("red".to_string())),
            ),
        ))));

        assert!(vision_tool().convert_log(log.clone()).await.is_none());

        // The image never enters the caller's context in prompt mode
        assert!(matches!(log.read().unwrap().data(), TenonLogData::Tool(_)));
    }

    fn context_with_signal() -> (ToolContext, Arc<ContinueSignal>) {
        let signal = Arc::new(ContinueSignal(AtomicBool::new(false)));
        let context = ToolContext::new().with_scope(signal.clone());
        (context, signal)
    }

    #[tokio::test]
    async fn test_result_missing_file_is_error_and_does_not_signal() {
        let (mut context, signal) = context_with_signal();
        let call = ReadImageCall {
            filepath: "/nonexistent/image.png".to_string(),
            prompt: None,
            worker_model: test_model(true),
        };
        assert!(call.result(&mut context).await.is_err());
        assert!(!signal.0.load(Ordering::SeqCst));
    }

    #[tokio::test]
    async fn test_result_url_fetches_and_base64_encodes() {
        let server = wiremock::MockServer::start().await;
        wiremock::Mock::given(wiremock::matchers::method("GET"))
            .respond_with(wiremock::ResponseTemplate::new(200).set_body_bytes(utils_test_png()))
            .mount(&server)
            .await;

        let (mut context, signal) = context_with_signal();
        let call = ReadImageCall {
            filepath: format!("{}/image.png", server.uri()),
            prompt: None,
            worker_model: test_model(true),
        };
        let output = call.result(&mut context).await.unwrap();
        match output.as_content() {
            [ToolResultContent::Image(image)] => {
                assert!(
                    matches!(image.data, DocumentSourceKind::Base64(_)),
                    "URL images must be base64-encoded, got {:?}",
                    image.data
                );
                assert_eq!(image.media_type, Some(rig::message::ImageMediaType::PNG));
            }
            other => panic!("expected a single image content, got {other:?}"),
        }
        assert!(signal.0.load(Ordering::SeqCst));
    }

    #[tokio::test]
    async fn test_image_url_is_base64_encoded_in_prompt_mode() {
        let server = wiremock::MockServer::start().await;
        wiremock::Mock::given(wiremock::matchers::method("GET"))
            .respond_with(wiremock::ResponseTemplate::new(200).set_body_bytes(utils_test_png()))
            .mount(&server)
            .await;

        let call = ReadImageCall {
            filepath: format!("{}/image.png", server.uri()),
            prompt: Some("What color?".to_string()),
            worker_model: test_model(true),
        };
        let image = call.image().await.unwrap();
        assert!(
            matches!(image.data, DocumentSourceKind::Base64(_)),
            "URL images must be base64-encoded, got {:?}",
            image.data
        );
        assert_eq!(image.media_type, Some(rig::message::ImageMediaType::PNG));
    }

    #[tokio::test]
    async fn test_result_url_fetch_failure_is_error_and_does_not_signal() {
        let server = wiremock::MockServer::start().await;
        wiremock::Mock::given(wiremock::matchers::method("GET"))
            .respond_with(wiremock::ResponseTemplate::new(404))
            .mount(&server)
            .await;

        let (mut context, signal) = context_with_signal();
        let call = ReadImageCall {
            filepath: format!("{}/missing.png", server.uri()),
            prompt: None,
            worker_model: test_model(true),
        };
        assert!(call.result(&mut context).await.is_err());
        assert!(!signal.0.load(Ordering::SeqCst));
    }

    #[tokio::test]
    async fn test_result_success_sets_continue_signal() {
        let (mut context, signal) = context_with_signal();
        let dir = std::env::temp_dir().join("tenon_test_read_image_signal");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("pixel.png");
        std::fs::write(&path, utils_test_png()).unwrap();

        let call = ReadImageCall {
            filepath: path.to_string_lossy().to_string(),
            prompt: None,
            worker_model: test_model(true),
        };
        let output = call.result(&mut context).await.unwrap();
        assert!(matches!(output.as_content(), [ToolResultContent::Image(_)]));
        assert!(signal.0.load(Ordering::SeqCst));
    }

    #[test]
    fn test_parameters_require_prompt_only_without_vision() {
        let vision_parameters = vision_tool().parameters();
        assert_eq!(
            vision_parameters["required"],
            serde_json::json!(["filepath"])
        );
        // `default: null` marks prompt as omittable for vision models
        assert!(
            vision_parameters["properties"]["prompt"]
                .get("default")
                .is_some()
        );

        let non_vision_parameters = ReadImage::NonVision {
            fallback_model: test_model(true),
        }
        .parameters();
        assert_eq!(
            non_vision_parameters["required"],
            serde_json::json!(["filepath", "prompt"])
        );
        assert!(
            non_vision_parameters["properties"]["prompt"]
                .get("default")
                .is_none()
        );
    }

    #[tokio::test]
    async fn test_init_call_uses_non_vision_variant_model_as_worker() {
        let tool = ReadImage::NonVision {
            fallback_model: test_model(true),
        };
        let call = tool
            .init_call(
                &mut ToolContext::new(),
                ReadImageArgs {
                    filepath: "./img/diagram.png".to_string(),
                    prompt: Some("What color?".to_string()),
                },
            )
            .await
            .expect("prompt mode should be allowed with a fallback model");
        assert_eq!(call.worker_model.id, "test");
    }

    #[test]
    fn test_description_adapts_to_vision_capability() {
        let vision_description = vision_tool().description();
        assert!(
            vision_description.contains("Omit `prompt`"),
            "vision model description should advertise the load mode"
        );

        let non_vision_description = ReadImage::NonVision {
            fallback_model: test_model(true),
        }
        .description();
        assert!(
            !non_vision_description.contains("Omit `prompt`"),
            "non-vision model description should not advertise the load mode, got: {non_vision_description}"
        );
        assert!(
            non_vision_description.contains("A subagent answers"),
            "non-vision model description should describe prompt mode, got: {non_vision_description}"
        );
    }

    #[tokio::test]
    async fn test_init_call_errors_when_prompt_missing_without_vision() {
        let tool = ReadImage::NonVision {
            fallback_model: test_model(true),
        };
        let err = tool
            .init_call(
                &mut ToolContext::new(),
                ReadImageArgs {
                    filepath: "./img/diagram.png".to_string(),
                    prompt: None,
                },
            )
            .await
            .err()
            .expect("prompt is required without vision");
        assert!(
            err.to_string()
                .contains("Failed to deserialize args: missing field `prompt`"),
            "got: {err}"
        );
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
