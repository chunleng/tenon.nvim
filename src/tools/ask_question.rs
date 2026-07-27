use rig::tool::{Tool, ToolError};
use serde::Deserialize;
use std::sync::{Arc, Mutex, Weak};

use crate::chat::{EventChannel, PendingAction};

/// Label for the option that lets the user defer the question back to chat
/// instead of answering inline. Selecting it returns an empty string, which
/// the chat loop interprets as a signal to stop the current chat.
pub const ANSWER_BY_CHAT: &str = "Answer by Chat..";

/// Result of a question action.
/// `Some(text)` = user selected/typed an answer, `None` = cancelled.
pub struct QuestionResult {
    pub response: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AskQuestionArgs {
    pub question: String,
    pub options: Vec<String>,
}

pub struct AskQuestion {
    pub event_channel: Weak<EventChannel<PendingAction>>,
}

impl Tool for AskQuestion {
    const NAME: &'static str = "ask_question";
    type Error = ToolError;
    type Args = AskQuestionArgs;
    type Output = String;

    fn description(&self) -> String {
        "Ask question with options and return the user's response. \
         Each question returns a single answer — it does not support multi-select. \
         Call this tool multiple times for more than one question."
            .to_string()
    }

    fn parameters(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "question": {
                    "type": "string",
                    "description": "Question to ask"
                },
                "options": {
                    "type": "array",
                    "items": {"type": "string"},
                    "description": "Answer choices. Prefix an option with '★' to mark it recommended."
                }
            },
            "required": ["question", "options"]
        })
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        let options = args.options;

        let (tx, rx) = tokio::sync::oneshot::channel::<QuestionResult>();

        if let Some(event_channel) = self.event_channel.upgrade() {
            event_channel.push(PendingAction::Question {
                question: args.question,
                options,
                response_tx: Arc::new(Mutex::new(Some(tx))),
            });
        } else {
            return Ok("User dismissed the question".to_string());
        }

        let result = rx.await;

        match result {
            Ok(result) => match result.response {
                Some(text) if text == ANSWER_BY_CHAT => Ok(
                    "<context>The user chose to answer via chat instead of selecting an option. \
                     Stop your response with \"I am listening\" and wait for the user's \
                     message. Do not call ask_question again.</context>"
                        .to_string(),
                ),
                Some(text) => Ok(text),
                None => Ok("User dismissed the question".to_string()),
            },
            Err(_) => Ok("User dismissed the question".to_string()),
        }
    }
}
