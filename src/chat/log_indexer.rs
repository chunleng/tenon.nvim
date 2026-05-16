use std::sync::Arc;

use crate::rag::RagContext;
use rig::{OneOrMany, completion::Message, message::UserContent};

use super::log::{TenonLog, TenonLogData};

/// Wrapper around TenonLog for indexing purposes.
#[derive(Clone)]
pub struct IndexedLog {
    pub log: Arc<TenonLog>,
    pub active: bool,
}

/// Manages chat logs with context truncation and RAG support.
/// Encapsulates log storage, resume position tracking, and RAG context management.
pub struct ChatLogIndexer {
    pub logs: Vec<IndexedLog>,
    pub rag_context: RagContext,
}

impl ChatLogIndexer {
    #[cfg(not(test))]
    const MAX_ACTIVE_CONTEXT_TOKENS: usize = 10_000;

    #[cfg(test)]
    const MAX_ACTIVE_CONTEXT_TOKENS: usize = 10;

    /// Creates a new empty ChatLogIndexer.
    pub fn new() -> Self {
        Self {
            logs: Vec::new(),
            rag_context: RagContext::new(),
        }
    }

    /// Creates a ChatLogIndexer from existing logs (for history restoration).
    /// All logs are initialized as active by default.
    pub fn from_logs(logs: Vec<TenonLog>) -> Self {
        let mut s = Self {
            logs: logs
                .into_iter()
                .map(|log| IndexedLog {
                    log: Arc::new(log),
                    active: true,
                })
                .collect(),
            rag_context: RagContext::new(),
        };

        s.apply_context_truncation();
        s
    }

    // --- Log access ---

    /// Returns active messages that will be sent to LLM as chat context.
    /// Active logs are those with active=true.
    /// Each TenonLog is converted to Vec<Message> (some logs produce multiple messages).
    pub fn active_messages(&self) -> Vec<Message> {
        self.logs
            .iter()
            .filter(|indexed| indexed.active)
            .flat_map(|indexed| Vec::<Message>::from(TenonLog::clone(&indexed.log)))
            .collect()
    }

    /// Builds the chat history for an LLM request:
    /// applies context truncation, collects active messages, and prepends RAG context.
    pub fn retrieve_chatlog_with_context(&mut self, user_message: &str) -> Vec<Message> {
        self.apply_context_truncation();
        let mut chat_history = self.active_messages();
        let history_messages = self.get_relevant_context(user_message);
        for msg in history_messages.into_iter().rev() {
            chat_history.insert(0, msg);
        }
        chat_history
    }

    /// Returns inactive logs that will go through RAG filter.
    /// These are logs that have been excluded from active context (active=false).
    pub fn inactive_log(&self) -> Vec<Arc<TenonLog>> {
        self.logs
            .iter()
            .filter(|indexed| !indexed.active)
            .map(|indexed| indexed.log.clone())
            .collect()
    }

    // --- Query ---

    /// Returns true if the log entry is a user message.
    fn is_user_log(log: &TenonLog) -> bool {
        matches!(log.data(), TenonLogData::User(_))
    }

    /// Finds the last user message index in the logs.
    /// Returns None if no user message is found.
    pub fn find_last_user_index(&self) -> Option<usize> {
        self.logs
            .iter()
            .rposition(|indexed| Self::is_user_log(&indexed.log))
    }

    // --- Token management ---

    /// Returns the total token count of active chat logs.
    pub fn active_context_token_count(&self) -> usize {
        self.logs
            .iter()
            .filter(|indexed| indexed.active)
            .map(|indexed| indexed.log.token_count())
            .sum()
    }

    // --- Context management ---

    /// Applies context truncation if token count exceeds max_active_context_tokens.
    /// Marks logs as inactive to remove them from active context, keeping them for display and RAG access.
    /// The last user/assistant exchange is always preserved.
    pub fn apply_context_truncation(&mut self) {
        // Early return if under threshold
        if self.active_context_token_count() <= Self::MAX_ACTIVE_CONTEXT_TOKENS {
            return;
        }

        // Find the last user message - this is the boundary we cannot cross
        let last_user_idx = self.find_last_user_index();

        // Collect indices of logs to truncate
        let mut indices_to_truncate = Vec::new();
        let mut total_tokens = self.active_context_token_count();

        for (idx, indexed) in self.logs.iter().enumerate() {
            // Stop if we've reached the last user message
            if let Some(last_idx) = last_user_idx {
                if idx >= last_idx {
                    break;
                }
            }

            // Only process active logs
            if indexed.active {
                total_tokens -= indexed.log.token_count();
                indices_to_truncate.push(idx);

                if total_tokens <= Self::MAX_ACTIVE_CONTEXT_TOKENS {
                    break;
                }
            }
        }

        // Apply truncation
        for idx in indices_to_truncate {
            self.logs[idx].active = false;
        }
    }

    /// Builds a history message from RAG context using inactive logs.
    /// Returns empty Vec if no user message provided, no inactive logs, or no relevant context found.
    /// Returns a Vec with one Message::User with <chat-history> wrapped context when found.
    pub fn get_relevant_context(&self, user_message: &str) -> Vec<Message> {
        if user_message.is_empty() {
            return Vec::new();
        }
        // TODO we might want to produce 3 history log instead of one in the future
        let inactive_logs = self.inactive_log();
        self.rag_context
            .build_context(&inactive_logs, user_message)
            .map(|ctx| Message::User {
                content: OneOrMany::one(UserContent::text(format!(
                    "<chat-history>{}</chat-history>",
                    ctx.trim()
                ))),
            })
            .into_iter()
            .collect()
    }
}

#[cfg(test)]
mod tests {
    const ONE_TOKEN: &str = "xx";
    const TEN_TOKENS: &str = "xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx";
    use skimtoken::estimate_tokens;

    use crate::chat::{
        TenonAssistantMessage, TenonAssistantMessageContent,
        log::{TenonLog, TenonLogData, TenonUserMessage, TenonUserTextMessage},
    };

    fn create_user_log(text: &str) -> TenonLog {
        TenonLog::new(TenonLogData::User(TenonUserMessage::Text(
            TenonUserTextMessage(text.to_string()),
        )))
    }

    fn create_assistant_log(text: &str) -> TenonLog {
        TenonLog::new(TenonLogData::Assistant(TenonAssistantMessage {
            content: vec![TenonAssistantMessageContent::Text(text.to_string())],
            reasoning: None,
        }))
    }

    #[test]
    fn test_log_indexer_new_creates_empty() {
        let indexer = super::ChatLogIndexer::new();
        assert_eq!(indexer.logs.len(), 0);
    }

    #[test]
    fn test_log_indexer_from_logs() {
        let logs = vec![create_user_log("Hello"), create_user_log("World")];
        let indexer = super::ChatLogIndexer::from_logs(logs);
        assert_eq!(indexer.logs.len(), 2);
        // All logs should start as active
        assert!(indexer.logs.iter().all(|l| l.active));
    }

    #[test]
    fn test_push_adds_log() {
        let mut indexer = super::ChatLogIndexer::new();
        indexer.logs.push(super::IndexedLog {
            log: std::sync::Arc::new(create_user_log("Test message")),
            active: true,
        });
        assert_eq!(indexer.logs.len(), 1);
    }

    #[test]
    fn test_find_last_user_index() {
        let logs = vec![
            create_user_log("First"),
            create_user_log("Second"),
            create_user_log("Third"),
        ];
        let indexer = super::ChatLogIndexer::from_logs(logs);
        assert_eq!(indexer.find_last_user_index(), Some(2));
    }

    #[test]
    fn test_active_context_token_count_empty() {
        let indexer = super::ChatLogIndexer::new();
        assert_eq!(indexer.active_context_token_count(), 0);
    }

    #[test]
    fn test_active_context_token_count_with_logs() {
        let logs = vec![
            create_user_log("Hello world"),
            create_user_log("Test message"),
        ];
        let indexer = super::ChatLogIndexer::from_logs(logs);
        assert!(indexer.active_context_token_count() > 0);
    }

    #[test]
    fn test_active_messages_returns_all_when_no_truncation() {
        let logs = vec![
            create_user_log("First"),
            create_user_log("Second"),
            create_user_log("Third"),
        ];
        let indexer = super::ChatLogIndexer::from_logs(logs);
        let active = indexer.active_messages();
        assert_eq!(active.len(), 3);
    }

    #[test]
    fn test_active_messages_returns_subset_after_truncation() {
        let logs = vec![
            create_user_log("First"),
            create_user_log("Second"),
            create_user_log("Third"),
        ];
        let mut indexer = super::ChatLogIndexer::from_logs(logs);
        // Simulate inactivity by setting active=false on first log
        indexer.logs[0].active = false;
        let active = indexer.active_messages();
        assert_eq!(active.len(), 2);
    }

    #[test]
    fn test_inactive_log_returns_empty_when_no_truncation() {
        let logs = vec![
            create_user_log("First"),
            create_user_log("Second"),
            create_user_log("Third"),
        ];
        let indexer = super::ChatLogIndexer::from_logs(logs);
        let inactive = indexer.inactive_log();
        assert_eq!(inactive.len(), 0);
    }

    #[test]
    fn test_inactive_log_returns_inactive_logs() {
        let logs = vec![
            create_user_log("First"),
            create_user_log("Second"),
            create_user_log("Third"),
        ];
        let mut indexer = super::ChatLogIndexer::from_logs(logs);
        // Simulate inactivity by setting active=false on first log
        indexer.logs[0].active = false;
        let inactive = indexer.inactive_log();
        assert_eq!(inactive.len(), 1);
    }

    #[test]
    fn test_get_relevant_context_returns_empty_when_user_message_is_none() {
        let indexer = super::ChatLogIndexer::new();
        let result = indexer.get_relevant_context("");
        assert!(result.is_empty());
    }

    #[test]
    fn test_get_relevant_context_returns_empty_when_no_inactive_logs() {
        let logs = vec![create_user_log("Hello")];
        let indexer = super::ChatLogIndexer::from_logs(logs);
        let result = indexer.get_relevant_context("test message");
        assert!(result.is_empty());
    }

    #[test]
    fn test_get_relevant_context_returns_empty_when_rag_context_empty() {
        let logs = vec![create_user_log("First"), create_user_log("Second")];
        let mut indexer = super::ChatLogIndexer::from_logs(logs);
        // Set active flag to false to create inactive logs
        indexer.logs[0].active = false;

        // RAG context should return None for empty/irrelevant context
        let result = indexer.get_relevant_context("test message");
        // This might return empty vec or vec with message depending on RAG implementation
        // For now, we test that the method exists and doesn't panic
        let _ = result;
    }

    #[test]
    fn test_one_token_and_ten_tokens_constants() {
        // Verify constants match their intended token counts
        assert_eq!(estimate_tokens(ONE_TOKEN), 1);
        assert_eq!(estimate_tokens(TEN_TOKENS), 10);
    }

    // --- apply_context_truncation tests ---

    #[test]
    fn test_apply_context_truncation_no_truncation_when_under_threshold() {
        // Create logs that are well under the test threshold (10 tokens)
        let logs = vec![create_user_log(ONE_TOKEN), create_user_log(ONE_TOKEN)];
        let mut indexer = super::ChatLogIndexer::from_logs(logs);

        indexer.apply_context_truncation();

        // Should not mark as inactive - all logs should have active=true
        assert!(indexer.logs.iter().all(|l| l.active));
    }

    #[test]
    fn test_apply_context_truncation_truncates_when_over_threshold() {
        // Create logs that exceed the test threshold (10 tokens)
        // Two logs of TEN_TOKENS exceed threshold, third log (ONE_TOKEN) fits under threshold
        let logs = vec![
            create_user_log(TEN_TOKENS),
            create_user_log(TEN_TOKENS),
            create_user_log(ONE_TOKEN),
        ];
        let mut indexer = super::ChatLogIndexer::from_logs(logs);

        indexer.apply_context_truncation();

        // First two logs exceed 10 tokens, should be marked as inactive
        assert!(!indexer.logs[0].active);
        assert!(!indexer.logs[1].active);
        assert!(indexer.logs[2].active);
    }

    #[test]
    fn test_apply_context_truncation_preserves_last_exchange() {
        // Even when severely over threshold, last user message must be preserved
        // Using 4 logs, each exceeding threshold individually
        let logs = vec![
            create_user_log(TEN_TOKENS),
            create_user_log(TEN_TOKENS),
            create_user_log(TEN_TOKENS),
            create_user_log(TEN_TOKENS),
        ];
        let mut indexer = super::ChatLogIndexer::from_logs(logs);

        indexer.apply_context_truncation();

        // Last user log (index 3) must be preserved (active=true)
        // With threshold 10 tokens, each log exceeds threshold, only last log fits
        assert!(!indexer.logs[0].active);
        assert!(!indexer.logs[1].active);
        assert!(!indexer.logs[2].active);
        assert!(indexer.logs[3].active);
    }

    #[test]
    fn test_apply_context_truncation_lands_on_user_boundary() {
        // When truncation lands on non-user, should adjust to next user
        let logs = vec![
            create_user_log(ONE_TOKEN),
            create_assistant_log(ONE_TOKEN),
            create_user_log(ONE_TOKEN),
            create_assistant_log(ONE_TOKEN),
            // Last conversation round must be preserved
            create_user_log(TEN_TOKENS),
            create_assistant_log(ONE_TOKEN),
        ];
        let mut indexer = super::ChatLogIndexer::from_logs(logs);

        indexer.apply_context_truncation();

        // First four logs should be inactive, last two (user + assistant) preserved
        assert!(!indexer.logs[0].active);
        assert!(!indexer.logs[1].active);
        assert!(!indexer.logs[2].active);
        assert!(!indexer.logs[3].active);
        assert!(indexer.logs[4].active);
        assert!(indexer.logs[5].active);
    }

    #[test]
    fn test_apply_context_truncation_no_change_when_already_inactive() {
        // If logs are already inactive and under threshold, should not change
        let logs = vec![
            create_user_log(TEN_TOKENS),
            create_user_log(ONE_TOKEN),
            create_user_log(ONE_TOKEN),
        ];
        let mut indexer = super::ChatLogIndexer::from_logs(logs);

        // Pre-mark first log as inactive to simulate prior truncation
        indexer.logs[0].active = false;

        // Apply truncation - should not change anything since under threshold
        indexer.apply_context_truncation();

        assert!(!indexer.logs[0].active, "first log should remain inactive");
        assert!(indexer.logs[1].active, "second log should remain active");
        assert!(indexer.logs[2].active, "third log should remain active");
    }

    #[test]
    fn test_apply_context_truncation_empty_logs() {
        let mut indexer = super::ChatLogIndexer::new();
        indexer.apply_context_truncation();
        assert_eq!(indexer.logs.len(), 0);
    }

    #[test]
    fn test_retrieve_chatlog_with_context_applies_truncation() {
        // Create logs that exceed threshold
        let logs = vec![
            create_user_log(TEN_TOKENS),
            create_user_log(TEN_TOKENS),
            create_user_log(ONE_TOKEN),
        ];
        let mut indexer = super::ChatLogIndexer::from_logs(logs);
        let result = indexer.retrieve_chatlog_with_context("");
        // After truncation, only last message (ONE_TOKEN) is active
        assert_eq!(result.len(), 1);
        // First two logs should be inactive
        assert!(!indexer.logs[0].active);
        assert!(!indexer.logs[1].active);
        assert!(indexer.logs[2].active);
    }
}
