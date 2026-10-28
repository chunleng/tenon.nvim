use rig::tool::{ToolContext, ToolExecutionError};
use serde::Deserialize;
use std::sync::{Arc, Mutex, Weak};

use crate::chat::{EventChannel, PendingAction};
use crate::tools::{ToolCore, ToolCoreCall};
use crate::ui::SelectOption;

/// Result of a question action.
/// `Some(text)` = user selected/typed an answer, `None` = cancelled.
pub struct QuestionResult {
    pub response: Option<String>,
}

/// A question asking the user to pick an option or type a free text answer.
#[derive(Clone)]
pub struct Question {
    pub question: String,
    pub open_ended_text: Option<String>,
    pub options: Vec<SelectOption>,
    pub response_tx: Arc<Mutex<Option<tokio::sync::oneshot::Sender<QuestionResult>>>>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AskQuestionArgs {
    pub question: String,
    #[serde(default)]
    pub open_ended_text: Option<String>,
    #[serde(default)]
    pub options: Option<Vec<SelectOption>>,
}

pub struct AskQuestion;

pub struct AskQuestionCall {
    response_rx: Option<tokio::sync::oneshot::Receiver<QuestionResult>>,
}

impl ToolCore for AskQuestion {
    fn name(&self) -> String {
        "ask_question".to_string()
    }
    type Error = ToolExecutionError;
    type Args = AskQuestionArgs;
    type Output = String;
    type Call = AskQuestionCall;

    fn description(&self) -> String {
        "Ask question with/without options. Returns user's response. \
         Single question, single answer only (no multi-select). \
         Offer options you can strongly recommend"
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
                "open_ended_text": {
                    "type": "string",
                    "description": "Text label for the open-ended option that is always appended \
                        for user to type in free-form answer"
                },
                "options": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "properties": {
                            "text": {
                                "type": "string",
                                "description": "Option text"
                            },
                            "recommended": {
                                "type": "boolean",
                                "description": "Mark option as good default choice",
                                "default": false
                            }
                        },
                        "required": ["text"]
                    },
                    "description": "Answer choices. Every option must be a genuine, distinct choice \
                        that fully answers the question on its own. Never add options that carry \
                        no answer and would force a follow-up question (e.g. 'Other', 'Decline', \
                        'None of these')"
                }
            },
            "required": ["question"]
        })
    }

    fn properties(&self, _args: &AskQuestionArgs) -> crate::chat::log::LogProperties {
        crate::chat::log::LogProperties {
            idempotent: false,
            important: false,
        }
    }

    async fn init_call(
        &self,
        context: &mut ToolContext,
        args: Self::Args,
    ) -> Result<Self::Call, Self::Error> {
        let event_channel = context
            .scope::<Weak<EventChannel<PendingAction>>>()
            .ok_or_else(|| ToolExecutionError::other("event channel not available"))?
            .upgrade();

        let (tx, rx) = tokio::sync::oneshot::channel::<QuestionResult>();

        if let Some(event_channel) = event_channel {
            let options = args.options.unwrap_or_default();
            event_channel.push(PendingAction::Question(Question {
                question: args.question,
                open_ended_text: args.open_ended_text,
                options,
                response_tx: Arc::new(Mutex::new(Some(tx))),
            }));
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
            open_ended_text: None,
            options: Some(
                options
                    .iter()
                    .map(|text| SelectOption {
                        text: (*text).to_string(),
                        recommended: false,
                        free_text: false,
                    })
                    .collect(),
            ),
        }
    }

    fn new_channel() -> Arc<EventChannel<PendingAction>> {
        Arc::new(EventChannel::new())
    }

    /// Responds to the front pending question with the given result.
    fn respond(channel: &EventChannel<PendingAction>, response: Option<String>) {
        let PendingAction::Question(question) = channel.peek().expect("question should be pushed")
        else {
            panic!("front pending action should be a question");
        };
        let tx = question
            .response_tx
            .lock()
            .unwrap()
            .take()
            .expect("tx available");
        assert!(tx.send(QuestionResult { response }).is_ok());
    }

    #[test]
    fn test_optional_fields_omitted_or_null_deserialize_to_none() {
        let missing: AskQuestionArgs =
            serde_json::from_str(r#"{"question": "Q?"}"#).expect("missing fields should parse");
        assert!(missing.options.is_none());
        assert!(missing.open_ended_text.is_none());

        let null: AskQuestionArgs =
            serde_json::from_str(r#"{"question": "Q?", "options": null, "open_ended_text": null}"#)
                .expect("null fields should parse");
        assert!(null.options.is_none());
        assert!(null.open_ended_text.is_none());
    }

    #[tokio::test]
    async fn test_open_ended_text_flows_to_question() {
        let channel = new_channel();
        let tool = AskQuestion;
        let mut context = ToolContext::new().with_scope(Arc::new(Arc::downgrade(&channel)));

        let args: AskQuestionArgs = serde_json::from_str(
            r#"{"question": "Confirm the implementation?",
                "options": [{"text": "Approve"}],
                "open_ended_text": "Decline and explain what's wrong.."}"#,
        )
        .expect("args with open_ended_text should parse");

        tool.init_call(&mut context, args)
            .await
            .expect("init_call should succeed");

        let PendingAction::Question(question) = channel.peek().expect("question should be pushed")
        else {
            panic!("front pending action should be a question");
        };
        assert_eq!(
            question.open_ended_text.as_deref(),
            Some("Decline and explain what's wrong..")
        );
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
            err.to_string().contains("event channel not available"),
            "unexpected error: {err}"
        );
    }

    #[tokio::test]
    async fn test_pushes_question_and_returns_answer() {
        let channel = new_channel();
        let tool = AskQuestion;
        let mut context = ToolContext::new().with_scope(Arc::new(Arc::downgrade(&channel)));

        let call = tool
            .init_call(&mut context, args("Pick one", &["a", "b"]))
            .await
            .expect("init_call should succeed");

        let PendingAction::Question(question) = channel.peek().expect("question should be pushed")
        else {
            panic!("front pending action should be a question");
        };
        assert_eq!(question.question, "Pick one");
        assert_eq!(question.options.len(), 2);
        assert_eq!(question.options[0].text, "a");

        respond(&channel, Some("a".to_string()));
        let output = call
            .result(&mut context)
            .await
            .expect("result should succeed");
        assert_eq!(output, "a");
    }

    #[tokio::test]
    async fn test_dismissed_response_returns_dismissed_message() {
        let channel = new_channel();
        let tool = AskQuestion;
        let mut context = ToolContext::new().with_scope(Arc::new(Arc::downgrade(&channel)));

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
        let mut context = ToolContext::new().with_scope(Arc::new(weak));

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
