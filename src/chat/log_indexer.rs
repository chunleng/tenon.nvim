use std::sync::{
    Arc, RwLock,
    atomic::{AtomicUsize, Ordering},
};

use crate::rag::RagContext;

use super::log::{TenonLog, TenonLogData};

/// Manages chat logs with context truncation and RAG support.
/// Encapsulates log storage, resume position tracking, and RAG context management.
#[derive(Clone)]
pub struct ChatLogIndexer {
    pub logs: Arc<RwLock<Vec<TenonLog>>>,
    pub resume_from: Arc<AtomicUsize>,
    pub rag_context: RagContext,
}

impl ChatLogIndexer {
    const MAX_ACTIVE_CONTEXT_TOKENS: usize = 10_000;
    /// Creates a new empty ChatLogIndexer.
    pub fn new() -> Self {
        Self {
            logs: Arc::new(RwLock::new(Vec::new())),
            resume_from: Arc::new(AtomicUsize::new(0)),
            rag_context: RagContext::new(),
        }
    }

    /// Creates a ChatLogIndexer from existing logs (for history restoration).
    pub fn from_logs(logs: Vec<TenonLog>) -> Self {
        Self {
            logs: Arc::new(RwLock::new(logs)),
            resume_from: Arc::new(AtomicUsize::new(0)),
            rag_context: RagContext::new(),
        }
    }

    // --- Log access ---

    /// Returns a clone of the logs Arc for external use.
    pub fn logs(&self) -> Arc<RwLock<Vec<TenonLog>>> {
        Arc::clone(&self.logs)
    }

    /// Returns active logs that will be sent to LLM as chat context.
    /// Active logs are from resume_from index to the end of logs.
    pub fn active_log(&self) -> Vec<TenonLog> {
        let resume_idx = self.resume_from.load(Ordering::SeqCst);
        if let Ok(logs) = self.logs.read() {
            logs.iter().skip(resume_idx).cloned().collect()
        } else {
            Vec::new()
        }
    }

    /// Returns inactive logs that will go through RAG filter.
    /// These are logs that have been truncated from active context (index 0 to resume_from).
    pub fn inactive_log(&self) -> Vec<TenonLog> {
        let resume_idx = self.resume_from.load(Ordering::SeqCst);
        if resume_idx == 0 {
            return Vec::new();
        }
        if let Ok(logs) = self.logs.read() {
            logs.iter().take(resume_idx).cloned().collect()
        } else {
            Vec::new()
        }
    }

    /// Returns the current resume_from index.
    pub fn resume_from(&self) -> Arc<AtomicUsize> {
        Arc::clone(&self.resume_from)
    }

    // --- Query ---

    /// Returns true if the log entry is a user message.
    fn is_user_log(log: &TenonLog) -> bool {
        matches!(log.data(), TenonLogData::User(_))
    }

    /// Finds the next user message index starting from `start_idx`.
    /// Returns None if no user message is found.
    pub fn find_next_user_index(&self, start_idx: usize) -> Option<usize> {
        if let Ok(logs) = self.logs.read() {
            logs.iter()
                .enumerate()
                .skip(start_idx)
                .find(|(_, log)| Self::is_user_log(log))
                .map(|(i, _)| i)
        } else {
            None
        }
    }

    /// Finds the last user message index in the logs.
    /// Returns None if no user message is found.
    pub fn find_last_user_index(&self) -> Option<usize> {
        if let Ok(logs) = self.logs.read() {
            logs.iter().rposition(|log| Self::is_user_log(log))
        } else {
            None
        }
    }

    // --- Token management ---

    /// Returns the total token count of chat logs from resume_from.
    pub fn active_context_token_count(&self) -> usize {
        let resume_idx = self.resume_from.load(Ordering::SeqCst);
        if let Ok(logs) = self.logs.read() {
            logs.iter()
                .skip(resume_idx)
                .map(|log| log.token_count())
                .sum()
        } else {
            0
        }
    }

    /// Recounts tokens for all logs (for history restore).
    pub fn recount_all_tokens(&self) {
        if let Ok(mut logs) = self.logs.write() {
            for log in logs.iter_mut() {
                log.recount_tokens();
            }
        }
    }

    // --- Context management ---

    /// Applies context truncation if token count exceeds max_active_context_tokens.
    /// Copies truncated logs to rag_logs and updates resume_from.
    /// Logs remain in self.logs for display purposes.
    /// The last user/assistant exchange is always preserved.
    pub fn apply_context_truncation(&self) {
        if let Ok(logs) = self.logs.read() {
            let current_resume = self.resume_from.load(Ordering::SeqCst);

            // Find the last user message - this is the boundary we cannot cross
            let last_user_idx = logs
                .iter()
                .rposition(|log| Self::is_user_log(log))
                .unwrap_or(0);

            // Minimum boundary: we must keep the last exchange
            let min_resume = last_user_idx.min(logs.len().saturating_sub(1));

            // Calculate tokens from current resume_from
            let mut total_tokens: usize = 0;
            for log in logs.iter().skip(current_resume) {
                total_tokens += log.token_count();
            }

            // If under threshold, no truncation needed
            if total_tokens <= Self::MAX_ACTIVE_CONTEXT_TOKENS {
                return;
            }

            // Need to truncate - find new resume_from
            let mut new_resume = current_resume;

            // Move resume_from forward until we're under threshold
            for log in logs.iter().skip(current_resume) {
                let log_tokens = log.token_count();
                total_tokens -= log_tokens;
                new_resume += 1;

                if total_tokens <= Self::MAX_ACTIVE_CONTEXT_TOKENS {
                    break;
                }
            }

            // Adjust to next user message if we landed on non-user
            if new_resume < logs.len() && !Self::is_user_log(&logs[new_resume]) {
                if let Some(user_idx) = self.find_next_user_index(new_resume) {
                    new_resume = user_idx;
                }
            }

            // Never truncate past the last exchange
            new_resume = new_resume.min(min_resume);

            // Only update if we're actually moving forward
            if new_resume <= current_resume {
                return;
            }

            // Update resume_from - logs remain in self.logs for display and RAG access
            // Embeddings cache in rag_context will be regenerated when needed
            self.resume_from.store(new_resume, Ordering::SeqCst);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::Ordering;

    use crate::chat::log::{TenonLog, TenonLogData, TenonUserMessage, TenonUserTextMessage};

    fn create_user_log(text: &str) -> TenonLog {
        TenonLog::new(TenonLogData::User(TenonUserMessage::Text(
            TenonUserTextMessage(text.to_string()),
        )))
    }

    #[test]
    fn test_log_indexer_new_creates_empty() {
        let indexer = super::ChatLogIndexer::new();
        assert_eq!(indexer.logs().read().unwrap().len(), 0);
        assert_eq!(indexer.resume_from().load(Ordering::SeqCst), 0);
    }

    #[test]
    fn test_log_indexer_from_logs() {
        let logs = vec![create_user_log("Hello"), create_user_log("World")];
        let indexer = super::ChatLogIndexer::from_logs(logs);
        assert_eq!(indexer.logs().read().unwrap().len(), 2);
        assert_eq!(indexer.resume_from().load(Ordering::SeqCst), 0);
    }

    #[test]
    fn test_push_adds_log() {
        let indexer = super::ChatLogIndexer::new();
        if let Ok(mut logs) = indexer.logs.write() {
            logs.push(create_user_log("Test message"));
        }
        assert_eq!(indexer.logs().read().unwrap().len(), 1);
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
        let indexer = super::ChatLogIndexer::from_logs(logs);
        let initial_count = indexer.active_context_token_count();
        indexer.recount_all_tokens();
        assert_eq!(indexer.active_context_token_count(), initial_count);
    }
}
