use std::sync::{Arc, PoisonError, RwLock};

use crate::agent::worker::simple::SimpleTenonWorkerAgent;
use crate::chat::log::TenonLogData;
use crate::chat::log::indexer::IndexedLog;
use crate::chat::log::window::LogWindow;
use crate::get_application_config;

/// Returns the text of the first user-like message in the logs: a typed
/// user message, or the answer of a question log when it comes first.
fn first_title_candidate(logs: &[IndexedLog]) -> Option<String> {
    logs.iter().find_map(|indexed| {
        let log = indexed.log.read().unwrap_or_else(PoisonError::into_inner);
        match log.data() {
            TenonLogData::User(crate::chat::TenonUserMessage::Text(msg)) => Some(msg.clone()),
            TenonLogData::Question(question) => Some(question.answer().to_string()),
            _ => None,
        }
    })
}

/// Handles title generation for chat sessions.
///
/// Owns the title state and a reference to the log indexer to read
/// the first user message. No cancellation token — generation is
/// guarded by checking `is_generating()` and whether a title exists.
pub struct TitleHandler {
    pub title: Arc<RwLock<Option<String>>>,
    log_window: Arc<RwLock<LogWindow>>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl TitleHandler {
    pub fn new(log_window: Arc<RwLock<LogWindow>>) -> Self {
        Self {
            title: Arc::new(RwLock::new(None)),
            log_window,
            thread: None,
        }
    }

    pub fn from_history(title: Option<String>, log_window: Arc<RwLock<LogWindow>>) -> Self {
        Self {
            title: Arc::new(RwLock::new(title)),
            log_window,
            thread: None,
        }
    }

    pub fn title(&self) -> Option<String> {
        self.title.read().ok().and_then(|t| t.clone())
    }

    pub fn is_generating(&self) -> bool {
        self.thread.as_ref().is_some_and(|t| !t.is_finished())
    }

    /// Generates a title from the first user message in the log indexer.
    /// Early-returns if a title is already set or generation is in progress.
    pub fn generate_title(&mut self) {
        if self.title.read().map(|t| t.is_some()).unwrap_or(false) {
            return;
        }

        if self.is_generating() {
            return;
        }

        let first_message = match self.log_window.read() {
            Ok(log_window) => first_title_candidate(&log_window.logs),
            Err(_) => None,
        };

        let first_message = match first_message {
            Some(msg) => msg,
            None => return,
        };

        let config = get_application_config();

        // The worker falls back to the default agent's model when `None`.
        let model = config.title.model.clone();

        let title = Arc::clone(&self.title);

        self.thread = Some(std::thread::spawn(move || {
            let rt = tokio::runtime::Runtime::new().unwrap();
            rt.block_on(async move {
                let directive_text = config.title.prompt.clone().unwrap_or_else(|| {
                    "Title generation\n
                            - Output title only\n
                            - 2-6 words\n
                            - Output \"Untitled\" when not enough context to form meaningful title\n\n
                            Example:\n
                            - prompt: Fix login bug in auth module\n
                              reply: Login bug fix\n
                            - prompt: Fix bug, add features for task manager\n
                              reply: Task manager code change\n
                            - prompt: Hey yo!\n
                              reply: Untitled\n
                            - prompt: X\n
                              reply: Untitled\n"
                        .to_string()
                });

                let worker = match SimpleTenonWorkerAgent::new(model, &directive_text) {
                    Ok(w) => w,
                    Err(_) => return,
                };

                match worker
                    .chat(format!("Generate title:\n```\n{}\n```", first_message))
                    .await
                {
                    Ok(generated) => {
                        let trimmed = generated.trim();
                        if !trimmed.is_empty()
                            && let Ok(mut t) = title.write()
                        {
                            *t = Some(
                                trimmed
                                    .lines()
                                    .collect::<Vec<_>>()
                                    .first()
                                    .map(|x| x.to_string())
                                    .unwrap_or("Untitled".to_string()),
                            );
                        }
                    }
                    Err(e) => {
                        crate::utils::GLOBAL_EXECUTION_HANDLER
                            .notify_on_main_thread(format!("Failed to generate title: {}", e), nvim_oxi::api::types::LogLevel::Error);
                    }
                }
            });
        }));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chat::log::TenonLog;
    use crate::chat::{
        TenonAssistantMessage, TenonAssistantMessageContent, TenonQuestionLog, TenonToolCall,
        TenonToolLog, TenonToolResult,
    };

    fn indexed(data: TenonLogData) -> IndexedLog {
        IndexedLog {
            log: Arc::new(RwLock::new(TenonLog::new(data))),
            active: true,
        }
    }

    fn assistant_log() -> TenonLogData {
        TenonLogData::Assistant(TenonAssistantMessage {
            content: vec![TenonAssistantMessageContent::Text(
                "assistant text".to_string(),
            )],
            reasoning: None,
            complete: true,
        })
    }

    fn question_log(question: &str, answer: &str) -> TenonLogData {
        TenonLogData::Question(TenonQuestionLog {
            tool_log: TenonToolLog {
                tool_call: TenonToolCall {
                    id: "call-1".to_string(),
                    internal_call_id: "call-1".to_string(),
                    item_id: None,
                    name: "ask_question".to_string(),
                    args: serde_json::json!({ "question": question }),
                },
                tool_result: Some(Ok(TenonToolResult::Text(rig::agent::Text::new(
                    answer.to_string(),
                )))),
                progress: vec![],
            },
        })
    }

    #[test]
    fn test_first_title_candidate_uses_question_answer_when_first() {
        // A first-position question log feeds its answer to title generation.
        let logs = vec![
            indexed(assistant_log()),
            indexed(question_log("Which module owns auth?", "auth module")),
        ];
        assert_eq!(
            first_title_candidate(&logs),
            Some("auth module".to_string())
        );

        // A typed user message before the question keeps precedence.
        let logs = vec![
            indexed(TenonLogData::User(crate::chat::TenonUserMessage::Text(
                "Fix login bug".to_string(),
            ))),
            indexed(question_log("Which module?", "auth module")),
        ];
        assert_eq!(
            first_title_candidate(&logs),
            Some("Fix login bug".to_string())
        );
    }

    #[test]
    fn test_first_title_candidate_none_without_user_like_message() {
        let logs = vec![indexed(assistant_log())];
        assert_eq!(first_title_candidate(&logs), None);
    }
}
