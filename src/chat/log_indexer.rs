use std::sync::Arc;

use crate::rag::RagContext;

use super::log::{TenonLog, TenonLogData};

/// Manages chat logs with context truncation and RAG support.
/// Encapsulates log storage, resume position tracking, and RAG context management.
pub struct ChatLogIndexer {
    pub logs: Vec<Arc<TenonLog>>,
    pub resume_from: usize,
    pub rag_context: RagContext,
}

impl ChatLogIndexer {
    const MAX_ACTIVE_CONTEXT_TOKENS: usize = 10_000;

    /// Creates a new empty ChatLogIndexer.
    pub fn new() -> Self {
        Self {
            logs: Vec::new(),
            resume_from: 0,
            rag_context: RagContext::new(),
        }
    }

    /// Creates a ChatLogIndexer from existing logs (for history restoration).
    pub fn from_logs(logs: Vec<TenonLog>) -> Self {
        Self {
            logs: logs.into_iter().map(Arc::new).collect(),
            resume_from: 0,
            rag_context: RagContext::new(),
        }
    }

    // --- Log access ---

    /// Returns active logs that will be sent to LLM as chat context.
    /// Active logs are from resume_from index to the end of logs.
    pub fn active_log(&self) -> Vec<Arc<TenonLog>> {
        self.logs.iter().skip(self.resume_from).cloned().collect()
    }

    /// Returns inactive logs that will go through RAG filter.
    /// These are logs that have been truncated from active context (index 0 to resume_from).
    pub fn inactive_log(&self) -> Vec<Arc<TenonLog>> {
        if self.resume_from == 0 {
            return Vec::new();
        }
        self.logs.iter().take(self.resume_from).cloned().collect()
    }

    /// Returns the current resume_from index.
    pub fn resume_from(&self) -> usize {
        self.resume_from
    }

    // --- Query ---

    /// Returns true if the log entry is a user message.
    fn is_user_log(log: &TenonLog) -> bool {
        matches!(log.data(), TenonLogData::User(_))
    }

    /// Finds the next user message index starting from `start_idx`.
    /// Returns None if no user message is found.
    pub fn find_next_user_index(&self, start_idx: usize) -> Option<usize> {
        self.logs
            .iter()
            .enumerate()
            .skip(start_idx)
            .find(|(_, log)| Self::is_user_log(log))
            .map(|(i, _)| i)
    }

    /// Finds the last user message index in the logs.
    /// Returns None if no user message is found.
    pub fn find_last_user_index(&self) -> Option<usize> {
        self.logs.iter().rposition(|log| Self::is_user_log(log))
    }

    // --- Token management ---

    /// Returns the total token count of chat logs from resume_from.
    pub fn active_context_token_count(&self) -> usize {
        self.logs
            .iter()
            .skip(self.resume_from)
            .map(|log| log.token_count())
            .sum()
    }

    /// Recounts tokens for all logs (for history restore).
    pub fn recount_all_tokens(&mut self) {
        for log in self.logs.iter_mut() {
            // Use Arc::make_mut to get mutable access if we have the only reference
            // This clones the inner TenonLog if there are other references
            let log_ref = Arc::make_mut(log);
            log_ref.recount_tokens();
        }
    }

    // --- Context management ---

    /// Applies context truncation if token count exceeds max_active_context_tokens.
    /// Copies truncated logs to rag_logs and updates resume_from.
    /// Logs remain in self.logs for display purposes.
    /// The last user/assistant exchange is always preserved.
    pub fn apply_context_truncation(&mut self) {
        let current_resume = self.resume_from;

        // Find the last user message - this is the boundary we cannot cross
        let last_user_idx = self
            .logs
            .iter()
            .rposition(|log| Self::is_user_log(log))
            .unwrap_or(0);

        // Minimum boundary: we must keep the last exchange
        let min_resume = last_user_idx.min(self.logs.len().saturating_sub(1));

        // Calculate tokens from current resume_from
        let mut total_tokens: usize = 0;
        for log in self.logs.iter().skip(current_resume) {
            total_tokens += log.token_count();
        }

        // If under threshold, no truncation needed
        if total_tokens <= Self::MAX_ACTIVE_CONTEXT_TOKENS {
            return;
        }

        // Need to truncate - find new resume_from
        let mut new_resume = current_resume;

        // Move resume_from forward until we're under threshold
        for log in self.logs.iter().skip(current_resume) {
            let log_tokens = log.token_count();
            total_tokens -= log_tokens;
            new_resume += 1;

            if total_tokens <= Self::MAX_ACTIVE_CONTEXT_TOKENS {
                break;
            }
        }

        // Adjust to next user message if we landed on non-user
        if new_resume < self.logs.len()
            && !Self::is_user_log(&self.logs[new_resume])
            && let Some(user_idx) = self.find_next_user_index(new_resume)
        {
            new_resume = user_idx;
        }

        // Never truncate past the last exchange
        new_resume = new_resume.min(min_resume);

        // Only update if we're actually moving forward
        if new_resume <= current_resume {
            return;
        }

        // Update resume_from - logs remain in self.logs for display and RAG access
        // Embeddings cache in rag_context will be regenerated when needed
        self.resume_from = new_resume;
    }
}

#[cfg(test)]
mod tests {
    use crate::chat::log::{TenonLog, TenonLogData, TenonUserMessage, TenonUserTextMessage};

    fn create_user_log(text: &str) -> TenonLog {
        TenonLog::new(TenonLogData::User(TenonUserMessage::Text(
            TenonUserTextMessage(text.to_string()),
        )))
    }

    #[test]
    fn test_log_indexer_new_creates_empty() {
        let indexer = super::ChatLogIndexer::new();
        assert_eq!(indexer.logs.len(), 0);
        assert_eq!(indexer.resume_from, 0);
    }

    #[test]
    fn test_log_indexer_from_logs() {
        let logs = vec![create_user_log("Hello"), create_user_log("World")];
        let indexer = super::ChatLogIndexer::from_logs(logs);
        assert_eq!(indexer.logs.len(), 2);
        assert_eq!(indexer.resume_from, 0);
    }

    #[test]
    fn test_push_adds_log() {
        let mut indexer = super::ChatLogIndexer::new();
        indexer
            .logs
            .push(std::sync::Arc::new(create_user_log("Test message")));
        assert_eq!(indexer.logs.len(), 1);
    }

    #[test]
    fn test_find_next_user_index() {
        let logs = vec![
            create_user_log("First"),
            create_user_log("Second"),
            create_user_log("Third"),
        ];
        let indexer = super::ChatLogIndexer::from_logs(logs);
        assert_eq!(indexer.find_next_user_index(0), Some(0));
        assert_eq!(indexer.find_next_user_index(1), Some(1));
        assert_eq!(indexer.find_next_user_index(3), None);
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
    fn test_recount_all_tokens() {
        let logs = vec![create_user_log("Hello"), create_user_log("World")];
        let mut indexer = super::ChatLogIndexer::from_logs(logs);
        let initial_count = indexer.active_context_token_count();
        indexer.recount_all_tokens();
        assert_eq!(indexer.active_context_token_count(), initial_count);
    }

    #[test]
    fn test_active_log_returns_all_when_resume_is_zero() {
        let logs = vec![
            create_user_log("First"),
            create_user_log("Second"),
            create_user_log("Third"),
        ];
        let indexer = super::ChatLogIndexer::from_logs(logs);
        let active = indexer.active_log();
        assert_eq!(active.len(), 3);
    }

    #[test]
    fn test_active_log_returns_subset_after_truncation() {
        let logs = vec![
            create_user_log("First"),
            create_user_log("Second"),
            create_user_log("Third"),
        ];
        let mut indexer = super::ChatLogIndexer::from_logs(logs);
        // Simulate truncation by setting resume_from
        indexer.resume_from = 1;
        let active = indexer.active_log();
        assert_eq!(active.len(), 2);
    }

    #[test]
    fn test_inactive_log_returns_empty_when_resume_is_zero() {
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
    fn test_inactive_log_returns_truncated_logs() {
        let logs = vec![
            create_user_log("First"),
            create_user_log("Second"),
            create_user_log("Third"),
        ];
        let mut indexer = super::ChatLogIndexer::from_logs(logs);
        // Simulate truncation by setting resume_from
        indexer.resume_from = 1;
        let inactive = indexer.inactive_log();
        assert_eq!(inactive.len(), 1);
    }
}
