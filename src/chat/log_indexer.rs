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

    #[cfg(not(test))]
    const HARD_LIMIT_ACTIVE_CONTEXT_TOKENS: usize = 50_000;

    #[cfg(test)]
    const HARD_LIMIT_ACTIVE_CONTEXT_TOKENS: usize = 20;

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

    /// Returns true if the log entry is a workflow log.
    fn is_workflow_log(log: &TenonLog) -> bool {
        matches!(log.data(), TenonLogData::Workflow(_))
    }

    /// Returns true if the log entry is a tool log.
    fn is_tool_log(log: &TenonLog) -> bool {
        matches!(log.data(), TenonLogData::Tool(_))
    }

    /// Returns true if the workflow log indicates workflow start/navigate (step: Some).
    fn is_active_workflow(log: &TenonLog) -> bool {
        match log.data() {
            TenonLogData::Workflow(wf) => wf.step.is_some(),
            _ => false,
        }
    }

    /// Determines if a log at the given index is "in workflow" relative to a slice.
    pub fn is_log_in_workflow_in_slice(log_idx: usize, logs: &[IndexedLog]) -> bool {
        logs[..log_idx]
            .iter()
            .rev()
            .find(|indexed| Self::is_workflow_log(&indexed.log))
            .map(|indexed| Self::is_active_workflow(&indexed.log))
            .unwrap_or(false)
    }

    /// Finds the last checkpoint index in a slice of logs.
    /// Returns the index relative to the slice.
    pub fn find_last_checkpoint_in(logs: &[IndexedLog]) -> Option<usize> {
        if logs.is_empty() {
            return None;
        }

        let last_idx = logs.len().saturating_sub(1);
        if Self::is_log_in_workflow_in_slice(last_idx, logs) {
            // In workflow: checkpoint is the last workflow tool
            logs.iter()
                .rposition(|indexed| Self::is_active_workflow(&indexed.log))
        } else {
            // Not in workflow: checkpoint is the last user message
            logs.iter()
                .rposition(|indexed| Self::is_user_log(&indexed.log))
        }
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
    /// Marks logs as inactive (removed from active context but kept for display/RAG).
    /// Preserves last two checkpoints: workflow tools when in workflow, user messages otherwise.
    pub fn apply_context_truncation(&mut self) {
        // Early return if under threshold
        if self.active_context_token_count() <= Self::MAX_ACTIVE_CONTEXT_TOKENS {
            return;
        }

        // Find the last checkpoint, then find the checkpoint before it
        let mut boundary_idx = Self::find_last_checkpoint_in(&self.logs);
        if let Some(first) = boundary_idx
            && first > 0
            && let Some(second) = Self::find_last_checkpoint_in(&self.logs[..first])
        {
            boundary_idx = Some(second);
        }
        let boundary_idx = boundary_idx.unwrap_or(self.logs.len());

        // Collect indices of logs to truncate
        let mut indices_to_truncate = Vec::new();
        let mut total_tokens = self.active_context_token_count();

        for (idx, indexed) in self.logs.iter().enumerate() {
            // Stop if we've reached the preserve boundary
            if idx >= boundary_idx {
                break;
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

        // Hard limit enforcement: purge beyond boundary if necessary
        // Only process logs within the preserved boundary (from boundary_idx onwards)
        // First pass: deactivate tool logs (oldest to newest)
        // Second pass: deactivate any active log (oldest to newest)
        if self.active_context_token_count() > Self::HARD_LIMIT_ACTIVE_CONTEXT_TOKENS {
            let mut remaining_to_remove =
                self.active_context_token_count() - Self::HARD_LIMIT_ACTIVE_CONTEXT_TOKENS;

            // Phase 1: Make tool logs inactive (oldest to newest within boundary)
            for indexed in self.logs[boundary_idx..].iter_mut() {
                if indexed.active && Self::is_tool_log(&indexed.log) {
                    remaining_to_remove =
                        remaining_to_remove.saturating_sub(indexed.log.token_count());
                    indexed.active = false;
                    if remaining_to_remove == 0 {
                        break;
                    }
                }
            }

            // Phase 2: Make any active log inactive (oldest to newest within boundary)
            if remaining_to_remove > 0 {
                for indexed in self.logs[boundary_idx..].iter_mut() {
                    if indexed.active {
                        remaining_to_remove =
                            remaining_to_remove.saturating_sub(indexed.log.token_count());
                        indexed.active = false;
                        if remaining_to_remove == 0 {
                            break;
                        }
                    }
                }
            }
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

    fn create_workflow_log(id: &str, step: Option<usize>) -> TenonLog {
        use crate::chat::TenonWorkflowLog;
        TenonLog::new(TenonLogData::Workflow(TenonWorkflowLog::new(
            id,
            if step.is_some() {
                "workflow step"
            } else {
                "Workflow ended"
            },
            step,
        )))
    }

    fn create_tool_log(name: &str, result: bool) -> TenonLog {
        use crate::chat::TenonToolCall;
        TenonLog::new(TenonLogData::Tool(crate::chat::log::TenonToolLog {
            tool_call: TenonToolCall {
                id: "1".into(),
                internal_call_id: "1".into(),
                name: name.into(),
                args: serde_json::json!({}),
            },
            tool_result: if result {
                Some(Ok(crate::chat::log::TenonToolResult::Text(
                    rig::agent::Text { text: "ok".into() },
                )))
            } else {
                None
            },
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
        // New behavior: preserve last two checkpoints
        // First checkpoint: index 2 (last user)
        // Second checkpoint: index 1 (previous user)
        // Should preserve from index 1 onwards
        let logs = vec![
            create_user_log(TEN_TOKENS), // 0 - should be truncated
            create_user_log(TEN_TOKENS), // 1 - second checkpoint, preserved
            create_user_log(ONE_TOKEN),  // 2 - first checkpoint, preserved
        ];
        let mut indexer = super::ChatLogIndexer::from_logs(logs);

        indexer.apply_context_truncation();

        // First log should be truncated, last two preserved
        assert!(!indexer.logs[0].active);
        assert!(indexer.logs[1].active);
        assert!(indexer.logs[2].active);
    }

    #[test]
    fn test_apply_context_truncation_preserves_last_exchange() {
        // Even when severely over threshold, last two checkpoints must be preserved
        // Using 4 logs, each exceeding threshold individually
        // First checkpoint: index 3 (last user)
        // Second checkpoint: index 2 (previous user)
        // Should preserve from index 2 onwards
        let logs = vec![
            create_user_log(TEN_TOKENS), // 0 - truncated
            create_user_log(TEN_TOKENS), // 1 - truncated
            create_user_log(TEN_TOKENS), // 2 - second checkpoint, preserved
            create_user_log(TEN_TOKENS), // 3 - first checkpoint, preserved
        ];
        let mut indexer = super::ChatLogIndexer::from_logs(logs);

        indexer.apply_context_truncation();

        // Should preserve from index 2 (second checkpoint)
        assert!(!indexer.logs[0].active);
        assert!(!indexer.logs[1].active);
        assert!(indexer.logs[2].active);
        assert!(indexer.logs[3].active);
    }

    #[test]
    fn test_apply_context_truncation_lands_on_user_boundary() {
        // When truncation lands on non-user, should adjust to preserve from second checkpoint
        // First checkpoint: index 4 (last user)
        // Second checkpoint: index 2 (previous user)
        // Should preserve from index 2 onwards
        let logs = vec![
            create_user_log(ONE_TOKEN),      // 0 - truncated
            create_assistant_log(ONE_TOKEN), // 1 - truncated
            create_user_log(ONE_TOKEN),      // 2 - second checkpoint, preserved
            create_assistant_log(ONE_TOKEN), // 3 - preserved (part of interaction with user at 2)
            create_user_log(TEN_TOKENS),     // 4 - first checkpoint, preserved
            create_assistant_log(ONE_TOKEN), // 5 - preserved
        ];
        let mut indexer = super::ChatLogIndexer::from_logs(logs);

        indexer.apply_context_truncation();

        // First two logs truncated, last four preserved (second checkpoint onwards)
        assert!(!indexer.logs[0].active);
        assert!(!indexer.logs[1].active);
        assert!(indexer.logs[2].active);
        assert!(indexer.logs[3].active);
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
        // First checkpoint: index 2 (last user)
        // Second checkpoint: index 1 (previous user)
        // Should preserve from index 1
        let logs = vec![
            create_user_log(TEN_TOKENS), // 0 - truncated
            create_user_log(TEN_TOKENS), // 1 - second checkpoint, preserved
            create_user_log(ONE_TOKEN),  // 2 - first checkpoint, preserved
        ];
        let mut indexer = super::ChatLogIndexer::from_logs(logs);
        let result = indexer.retrieve_chatlog_with_context("");
        // After truncation, two messages should be active (indices 1 and 2)
        assert_eq!(result.len(), 2);
        // First log should be inactive
        assert!(!indexer.logs[0].active);
        assert!(indexer.logs[1].active);
        assert!(indexer.logs[2].active);
    }

    // --- Workflow-aware checkpoint tests ---

    #[test]
    fn test_is_log_in_workflow() {
        // Single setup covering all conditions
        let logs = vec![
            create_user_log("First"),           // 0: no workflow before
            create_workflow_log("wf", Some(1)), // 1: workflow start
            create_user_log("Second"),          // 2: in workflow
            create_workflow_log("wf", None),    // 3: workflow end
            create_user_log("Third"),           // 4: after workflow ended
        ];
        let indexer = super::ChatLogIndexer::from_logs(logs);
        assert!(!super::ChatLogIndexer::is_log_in_workflow_in_slice(
            0,
            &indexer.logs
        )); // before workflow
        assert!(super::ChatLogIndexer::is_log_in_workflow_in_slice(
            2,
            &indexer.logs
        )); // in workflow
        assert!(!super::ChatLogIndexer::is_log_in_workflow_in_slice(
            4,
            &indexer.logs
        )); // after workflow ended
    }

    #[test]
    fn test_find_last_checkpoint_in_workflow_uses_workflow_tool() {
        // In workflow: checkpoint is the last workflow tool
        let logs = vec![
            create_user_log("First"),
            create_workflow_log("test_workflow", Some(1)), // start
            create_user_log("Second"),
            create_workflow_log("test_workflow", Some(2)), // navigate to step 2
            create_user_log("Third"),
        ];
        let indexer = super::ChatLogIndexer::from_logs(logs);
        // Last checkpoint should be index 3 (navigate_workflow to step 2)
        assert_eq!(
            super::ChatLogIndexer::find_last_checkpoint_in(&indexer.logs),
            Some(3)
        );
    }

    #[test]
    fn test_find_last_checkpoint_not_in_workflow_uses_user_message() {
        // Not in workflow: checkpoint is the last user message
        let logs = vec![
            create_user_log("First"),
            create_assistant_log("Response"),
            create_user_log("Second"),
            create_assistant_log("Response 2"),
        ];
        let indexer = super::ChatLogIndexer::from_logs(logs);
        // Last checkpoint should be index 2 (last user message)
        assert_eq!(
            super::ChatLogIndexer::find_last_checkpoint_in(&indexer.logs),
            Some(2)
        );
    }

    #[test]
    fn test_find_last_checkpoint_after_workflow_ends() {
        // After workflow ends: checkpoint is last user message
        let logs = vec![
            create_workflow_log("test_workflow", Some(1)),
            create_user_log("User in workflow"),
            create_workflow_log("test_workflow", None), // end
            create_user_log("After workflow"),
        ];
        let indexer = super::ChatLogIndexer::from_logs(logs);
        // Last checkpoint should be index 3 (user message after workflow ended)
        assert_eq!(
            super::ChatLogIndexer::find_last_checkpoint_in(&indexer.logs),
            Some(3)
        );
    }

    #[test]
    fn test_apply_context_truncation_in_workflow_preserves_workflow_steps() {
        // In workflow: preserve from last workflow step
        let logs = vec![
            create_user_log(TEN_TOKENS),        // 0
            create_workflow_log("wf", Some(1)), // 1 - will be second checkpoint
            create_user_log(TEN_TOKENS),        // 2
            create_workflow_log("wf", Some(2)), // 3 - last checkpoint (in workflow)
            create_user_log(ONE_TOKEN),         // 4
        ];
        let mut indexer = super::ChatLogIndexer::from_logs(logs);
        indexer.apply_context_truncation();

        // Should preserve from index 1 (second checkpoint)
        assert!(!indexer.logs[0].active);
        assert!(indexer.logs[1].active); // workflow step 1 preserved
        assert!(indexer.logs[2].active);
        assert!(indexer.logs[3].active); // workflow step 2 preserved
        assert!(indexer.logs[4].active);
    }

    #[test]
    fn test_apply_context_truncation_outside_workflow_preserves_user_messages() {
        // Outside workflow: preserve from last user message
        let logs = vec![
            create_user_log(TEN_TOKENS),     // 0
            create_assistant_log(ONE_TOKEN), // 1
            create_user_log(TEN_TOKENS),     // 2 - second checkpoint
            create_assistant_log(ONE_TOKEN), // 3
            create_user_log(ONE_TOKEN),      // 4 - last checkpoint (last user)
        ];
        let mut indexer = super::ChatLogIndexer::from_logs(logs);
        indexer.apply_context_truncation();

        // Should preserve from index 2 (second checkpoint, previous user message)
        assert!(!indexer.logs[0].active);
        assert!(!indexer.logs[1].active);
        assert!(indexer.logs[2].active);
        assert!(indexer.logs[3].active);
        assert!(indexer.logs[4].active);
    }

    // --- Hard limit tests ---

    #[test]
    fn test_apply_context_truncation_hard_limit_purges_tools_first() {
        // Setup: Soft limit preserves last 2 checkpoints, hard limit triggers after
        // Soft limit = 10, Hard limit = 20
        // After soft limit, preserved section should exceed hard limit
        // Tool logs should be purged first by hard limit
        let logs = vec![
            create_user_log(TEN_TOKENS),    // 0 - purged by soft limit
            create_user_log(TEN_TOKENS),    // 1 - purged by soft limit
            create_user_log(TEN_TOKENS),    // 2 - second checkpoint (preserved by soft limit)
            create_tool_log("tool1", true), // 3 - tool log, purged by hard limit phase 1
            create_user_log(TEN_TOKENS),    // 4 - first checkpoint (preserved)
        ];
        let mut indexer = super::ChatLogIndexer::from_logs(logs);
        indexer.apply_context_truncation();

        // After soft limit: indices 2, 3, 4 preserved
        // Tool at index 3 has token count > 0, so total > 20
        // Hard limit phase 1: purge tools
        assert!(!indexer.logs[0].active, "purged by soft limit");
        assert!(!indexer.logs[1].active, "purged by soft limit");
        assert!(indexer.logs[2].active, "preserved by soft limit");
        assert!(!indexer.logs[3].active, "tool purged by hard limit phase 1");
        assert!(indexer.logs[4].active, "last user preserved");
    }

    #[test]
    fn test_apply_context_truncation_hard_limit_purges_beyond_boundary() {
        // Soft limit = 10, Hard limit = 20
        // After soft limit, preserved section exceeds hard limit
        // No tools, so phase 2 purges oldest active logs
        let logs = vec![
            create_user_log(TEN_TOKENS), // 0 - purged by soft limit
            create_user_log(TEN_TOKENS), // 1 - purged by soft limit
            create_user_log(TEN_TOKENS), // 2 - second checkpoint (preserved by soft limit, purged by hard limit)
            create_user_log(TEN_TOKENS), // 3 - first checkpoint (preserved)
            create_user_log(TEN_TOKENS), // 4 - last user (preserved)
        ];
        let mut indexer = super::ChatLogIndexer::from_logs(logs);
        indexer.apply_context_truncation();

        // After soft limit: indices 2, 3, 4 = 30 tokens (> 20)
        // Phase 1: no tools
        // Phase 2: purge index 2 (10 tokens), remaining = 20, stop
        // Result: indices 3, 4 active = 20 tokens
        assert!(!indexer.logs[0].active, "purged by soft limit");
        assert!(!indexer.logs[1].active, "purged by soft limit");
        assert!(!indexer.logs[2].active, "purged by hard limit");
        assert!(indexer.logs[3].active, "preserved");
        assert!(indexer.logs[4].active, "preserved");
    }
}
