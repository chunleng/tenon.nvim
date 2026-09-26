use rig::tool::{ToolContext, ToolExecutionError};
use serde::Deserialize;
use std::sync::{Arc, Mutex, Weak};

use crate::chat::{EventChannel, PendingAction};
use crate::tools::{ToolCore, ToolCoreCall};

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
    pub options: Vec<AskQuestionOption>,
}

/// An answer choice.
#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AskQuestionOption {
    pub text: String,
    #[serde(default)]
    pub recommended: bool,
}

pub struct AskQuestion;

pub struct AskQuestionCall {
    response_rx: Option<tokio::sync::oneshot::Receiver<QuestionResult>>,
}

impl ToolCore for AskQuestion {
    const NAME: &'static str = "ask_question";
    type Error = ToolExecutionError;
    type Args = AskQuestionArgs;
    type Output = String;
    type Call = AskQuestionCall;

    fn description(&self) -> String {
        "Ask question with options and return the user's response. \
         Single answer only (no multi-select); call multiple times for more questions. \
         Use when you can either enumerate options that cover the likely answers, \
         or offer options you can strongly recommend. \
         If neither holds, ask an open-ended question in chat instead."
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
                    "items": {
                        "type": "object",
                        "properties": {
                            "text": {"type": "string"},
                            "recommended": {
                                "type": "boolean",
                                "description": "Mark option as good default choice",
                                "default": false
                            }
                        },
                        "required": ["text"]
                    },
                    "description": "Answer choices. Every option must be a genuine, distinct choice. \
                        An \"Answer by Chat..\" option is appended automatically, so the user \
                        can always answer in chat. Never add an option that just leads back \
                        to typing (e.g. \"Something else\", \"Others\")"
                }
            },
            "required": ["question", "options"]
        })
    }

    async fn init_call(
        &self,
        context: &mut ToolContext,
        args: Self::Args,
    ) -> Result<Self::Call, Self::Error> {
        let event_channel = context
            .require::<Weak<EventChannel<PendingAction>>>()
            .map_err(ToolExecutionError::from_error)?;

        let (tx, rx) = tokio::sync::oneshot::channel::<QuestionResult>();

        if let Some(event_channel) = event_channel.upgrade() {
            event_channel.push(PendingAction::Question {
                question: args.question,
                options: args.options,
                response_tx: Arc::new(Mutex::new(Some(tx))),
            });
            Ok(AskQuestionCall {
                response_rx: Some(rx),
            })
        } else {
            Ok(AskQuestionCall { response_rx: None })
        }
    }
}

impl ToolCoreCall for AskQuestionCall {
    type Output = String;
    type Error = ToolExecutionError;

    async fn result(self, _context: &mut ToolContext) -> Result<Self::Output, Self::Error> {
        let Some(rx) = self.response_rx else {
            return Ok("User dismissed the question".to_string());
        };

        match rx.await {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::ToolCore;

    fn args(question: &str, options: &[&str]) -> AskQuestionArgs {
        AskQuestionArgs {
            question: question.to_string(),
            options: options
                .iter()
                .map(|text| AskQuestionOption {
                    text: (*text).to_string(),
                    recommended: false,
                })
                .collect(),
        }
    }

    fn new_channel() -> Arc<EventChannel<PendingAction>> {
        Arc::new(EventChannel::new())
    }

    /// Responds to the front pending question with the given result.
    fn respond(channel: &EventChannel<PendingAction>, response: Option<String>) {
        let pending = channel.peek().expect("question should be pushed");
        let PendingAction::Question { response_tx, .. } = pending;
        let tx = response_tx.lock().unwrap().take().expect("tx available");
        assert!(tx.send(QuestionResult { response }).is_ok());
    }

    #[tokio::test]
    async fn test_missing_context_returns_error() {
        let tool = AskQuestion;
        let mut context = ToolContext::new();

        let result = tool.init_call(&mut context, args("Q?", &["a"])).await;

        let err = result
            .map(|_| ())
            .expect_err("missing context value must error");
        assert!(
            err.to_string().contains("was not found"),
            "unexpected error: {err}"
        );
    }

    #[tokio::test]
    async fn test_pushes_question_and_returns_answer() {
        let channel = new_channel();
        let tool = AskQuestion;
        let mut context = ToolContext::new();
        context.insert(Arc::downgrade(&channel));

        let call = tool
            .init_call(&mut context, args("Pick one", &["a", "b"]))
            .await
            .expect("init_call should succeed");

        match channel.peek().expect("question should be pushed") {
            PendingAction::Question {
                question, options, ..
            } => {
                assert_eq!(question, "Pick one");
                assert_eq!(options.len(), 2);
                assert_eq!(options[0].text, "a");
            }
        }

        respond(&channel, Some("a".to_string()));
        let output = call
            .result(&mut context)
            .await
            .expect("result should succeed");
        assert_eq!(output, "a");
    }

    #[tokio::test]
    async fn test_answer_by_chat_returns_context_message() {
        let channel = new_channel();
        let tool = AskQuestion;
        let mut context = ToolContext::new();
        context.insert(Arc::downgrade(&channel));

        let call = tool
            .init_call(&mut context, args("Q?", &["a"]))
            .await
            .expect("init_call should succeed");

        respond(&channel, Some(ANSWER_BY_CHAT.to_string()));
        let output = call
            .result(&mut context)
            .await
            .expect("result should succeed");
        assert!(
            output.contains("answer via chat"),
            "unexpected output: {output}"
        );
    }

    #[tokio::test]
    async fn test_dismissed_response_returns_dismissed_message() {
        let channel = new_channel();
        let tool = AskQuestion;
        let mut context = ToolContext::new();
        context.insert(Arc::downgrade(&channel));

        let call = tool
            .init_call(&mut context, args("Q?", &["a"]))
            .await
            .expect("init_call should succeed");

        respond(&channel, None);
        let output = call
            .result(&mut context)
            .await
            .expect("result should succeed");
        assert_eq!(output, "User dismissed the question");
    }

    #[tokio::test]
    async fn test_dropped_channel_returns_dismissed_message() {
        let channel = new_channel();
        let weak = Arc::downgrade(&channel);
        drop(channel);

        let tool = AskQuestion;
        let mut context = ToolContext::new();
        context.insert(weak);

        let call = tool
            .init_call(&mut context, args("Q?", &["a"]))
            .await
            .expect("init_call should succeed with a dead channel");

        let output = call
            .result(&mut context)
            .await
            .expect("result should succeed");
        assert_eq!(output, "User dismissed the question");
    }
}
