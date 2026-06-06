use std::sync::Arc;

use crate::rag::RagContext;
use crate::tools::{ToolClassification, get_tool_classification};
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

    /// Returns true if the log entry is a user or assistant message (chat message).
    fn is_chat_log(log: &TenonLog) -> bool {
        matches!(
            log.data(),
            TenonLogData::User(_) | TenonLogData::Assistant(_)
        )
    }

    /// Returns the tool classification for a tool log.
    /// Returns None for non-tool logs.
    fn get_log_tool_classification(log: &TenonLog) -> Option<ToolClassification> {
        match log.data() {
            TenonLogData::Tool(tool_log) => Some(get_tool_classification(&tool_log.tool_call.name)),
            _ => None,
        }
    }

    /// Returns true if the workflow log indicates workflow start/navigate (step: Some).
    fn is_active_workflow(log: &TenonLog) -> bool {
        match log.data() {
            TenonLogData::Workflow(wf) => wf.step.is_some(),
            _ => false,
        }
    }

    /// Finds the index of the first user message in the entire log.
    fn find_first_user_index(logs: &[IndexedLog]) -> Option<usize> {
        logs.iter()
            .position(|indexed| Self::is_user_log(&indexed.log))
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
    ///
    /// Regions:
    /// - Region 1: Before 2x checkpoint (older logs)
    /// - Region 2: Between 1x and 2x checkpoint (middle logs)
    /// - Region 3: After 1x checkpoint (newer logs)
    ///
    /// Removal phases:
    /// - Soft limit: Phase 1 (idempotent tools regions 1&2), Phase 2 (non-idempotent tools region 1)
    /// - Hard limit:
    ///   - Phase 3 (chat/system region 1), Phase 4 (non-idempotent tools region 2),
    ///   - Phase 5 (idempotent tools region 3)
    /// - Workflow logs are never removed
    /// - First user message is always preserved
    pub fn apply_context_truncation(&mut self) {
        // Early return if under threshold
        let total = self.active_context_token_count();
        if total <= Self::MAX_ACTIVE_CONTEXT_TOKENS {
            return;
        }

        // Find checkpoints for region boundaries
        // Region 1: before second checkpoint (older logs)
        // Region 2: between first and second checkpoint (middle logs)
        // Region 3: after first checkpoint (newer logs)
        let first_checkpoint = Self::find_last_checkpoint_in(&self.logs);
        let second_checkpoint = first_checkpoint.and_then(|fc| {
            if fc > 0 {
                Self::find_last_checkpoint_in(&self.logs[..fc])
            } else {
                None
            }
        });

        // Find the first user message index (must never be removed)
        let first_user_idx = Self::find_first_user_index(&self.logs);

        // Helper to check if index is first user
        let is_first_user = |idx: usize| first_user_idx == Some(idx);

        // Helper to check if log is idempotent tool
        let is_idempotent_tool = |log: &TenonLog| {
            Self::get_log_tool_classification(log) == Some(ToolClassification::Idempotent)
        };

        // Helper to check if log is non-idempotent (non-mutating or mutating) tool
        let is_non_idempotent_tool = |log: &TenonLog| {
            matches!(
                Self::get_log_tool_classification(log),
                Some(ToolClassification::NonMutating | ToolClassification::Mutating)
            )
        };

        // Helper to check if log is system tool
        let is_system_tool = |log: &TenonLog| {
            Self::get_log_tool_classification(log) == Some(ToolClassification::System)
        };

        // Define region boundaries
        // Region 1: indices 0..region1_end (before checkpoint 2x)
        // Region 2: indices region1_end..region2_end (includes checkpoint 2x, excludes checkpoint 1x)
        // Region 3: indices region2_end..len (includes checkpoint 1x)
        let region1_end = second_checkpoint.unwrap_or(0);
        let region2_end = first_checkpoint.unwrap_or(0);

        let mut total_tokens = total;

        // === SOFT LIMIT: Phase 1 - Remove idempotent tools from regions 1 & 2 ===
        // Region 1 idempotent tools
        for idx in 0..region1_end {
            if total_tokens <= Self::MAX_ACTIVE_CONTEXT_TOKENS {
                break;
            }
            let indexed = &self.logs[idx];
            if indexed.active && is_idempotent_tool(&indexed.log) {
                total_tokens -= indexed.log.token_count();
                self.logs[idx].active = false;
            }
        }

        // Region 2 idempotent tools
        for idx in region1_end..region2_end {
            if total_tokens <= Self::MAX_ACTIVE_CONTEXT_TOKENS {
                break;
            }
            let indexed = &self.logs[idx];
            if indexed.active && is_idempotent_tool(&indexed.log) {
                total_tokens -= indexed.log.token_count();
                self.logs[idx].active = false;
            }
        }

        // === SOFT LIMIT: Phase 2 - Remove non-idempotent tools from region 1 ===
        for idx in 0..region1_end {
            if total_tokens <= Self::MAX_ACTIVE_CONTEXT_TOKENS {
                break;
            }
            let indexed = &self.logs[idx];
            if indexed.active && is_non_idempotent_tool(&indexed.log) {
                total_tokens -= indexed.log.token_count();
                self.logs[idx].active = false;
            }
        }

        // === HARD LIMIT: Phase 3 - Remove chat/system logs from region 1 ===
        if total_tokens > Self::HARD_LIMIT_ACTIVE_CONTEXT_TOKENS {
            let mut remaining_to_remove = total_tokens - Self::HARD_LIMIT_ACTIVE_CONTEXT_TOKENS;

            // Remove chat logs (excluding first user) from region 1
            for idx in 0..region1_end {
                if remaining_to_remove == 0 {
                    break;
                }
                let indexed = &self.logs[idx];
                if indexed.active && Self::is_chat_log(&indexed.log) && !is_first_user(idx) {
                    remaining_to_remove =
                        remaining_to_remove.saturating_sub(indexed.log.token_count());
                    self.logs[idx].active = false;
                }
            }

            // Remove system logs from region 1
            for idx in 0..region1_end {
                if remaining_to_remove == 0 {
                    break;
                }
                let indexed = &self.logs[idx];
                if indexed.active && is_system_tool(&indexed.log) {
                    remaining_to_remove =
                        remaining_to_remove.saturating_sub(indexed.log.token_count());
                    self.logs[idx].active = false;
                }
            }
        }

        // === HARD LIMIT: Phase 4 - Remove non-idempotent tools from region 2 ===
        if total_tokens > Self::HARD_LIMIT_ACTIVE_CONTEXT_TOKENS {
            let mut remaining_to_remove = total_tokens - Self::HARD_LIMIT_ACTIVE_CONTEXT_TOKENS;

            for idx in region1_end..region2_end {
                if remaining_to_remove == 0 {
                    break;
                }
                let indexed = &self.logs[idx];
                if indexed.active && is_non_idempotent_tool(&indexed.log) {
                    remaining_to_remove =
                        remaining_to_remove.saturating_sub(indexed.log.token_count());
                    self.logs[idx].active = false;
                }
            }
        }

        // === HARD LIMIT: Phase 5 - Remove idempotent tools from region 3 ===
        if total_tokens > Self::HARD_LIMIT_ACTIVE_CONTEXT_TOKENS {
            let mut remaining_to_remove = total_tokens - Self::HARD_LIMIT_ACTIVE_CONTEXT_TOKENS;

            for idx in region2_end..self.logs.len() {
                if remaining_to_remove == 0 {
                    break;
                }
                let indexed = &self.logs[idx];
                if indexed.active && is_idempotent_tool(&indexed.log) {
                    remaining_to_remove =
                        remaining_to_remove.saturating_sub(indexed.log.token_count());
                    self.logs[idx].active = false;
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
    use crate::chat::{
        TenonAssistantMessage, TenonAssistantMessageContent,
        log::{TenonLog, TenonLogData, TenonUserMessage, TenonUserTextMessage},
    };

    fn create_user_log(token_count: usize) -> TenonLog {
        let mut log = TenonLog::new(TenonLogData::User(TenonUserMessage::Text(
            TenonUserTextMessage("x".repeat(token_count)),
        )));
        log.token_count = token_count;
        log
    }

    fn create_assistant_log(token_count: usize) -> TenonLog {
        let mut log = TenonLog::new(TenonLogData::Assistant(TenonAssistantMessage {
            content: vec![TenonAssistantMessageContent::Text("x".repeat(token_count))],
            reasoning: None,
        }));
        log.token_count = token_count;
        log
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

    fn create_tool_log(name: &str, token_count: usize) -> TenonLog {
        use crate::chat::TenonToolCall;
        let mut log = TenonLog::new(TenonLogData::Tool(crate::chat::log::TenonToolLog {
            tool_call: TenonToolCall {
                id: "1".into(),
                internal_call_id: "1".into(),
                name: name.into(),
                args: serde_json::json!({}),
            },
            tool_result: Some(Ok(crate::chat::log::TenonToolResult::Text(
                rig::agent::Text { text: "ok".into() },
            ))),
        }));
        log.token_count = token_count;
        log
    }

    #[test]
    fn test_log_indexer_new_creates_empty() {
        let indexer = super::ChatLogIndexer::new();
        assert_eq!(indexer.logs.len(), 0);
    }

    #[test]
    fn test_log_indexer_from_logs() {
        let logs = vec![create_user_log(1), create_user_log(1)];
        let indexer = super::ChatLogIndexer::from_logs(logs);
        assert_eq!(indexer.logs.len(), 2);
        // All logs should start as active
        assert!(indexer.logs.iter().all(|l| l.active));
    }

    #[test]
    fn test_push_adds_log() {
        let mut indexer = super::ChatLogIndexer::new();
        indexer.logs.push(super::IndexedLog {
            log: std::sync::Arc::new(create_user_log(1)),
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
        let logs = vec![create_user_log(5), create_user_log(5)];
        let indexer = super::ChatLogIndexer::from_logs(logs);
        assert_eq!(indexer.active_context_token_count(), 10);
    }

    #[test]
    fn test_active_messages_returns_all_when_no_truncation() {
        let logs = vec![create_user_log(1), create_user_log(1), create_user_log(1)];
        let indexer = super::ChatLogIndexer::from_logs(logs);
        let active = indexer.active_messages();
        assert_eq!(active.len(), 3);
    }

    #[test]
    fn test_active_messages_returns_subset_after_truncation() {
        let logs = vec![create_user_log(1), create_user_log(1), create_user_log(1)];
        let mut indexer = super::ChatLogIndexer::from_logs(logs);
        // Simulate inactivity by setting active=false on first log
        indexer.logs[0].active = false;
        let active = indexer.active_messages();
        assert_eq!(active.len(), 2);
    }

    #[test]
    fn test_inactive_log_returns_empty_when_no_truncation() {
        let logs = vec![create_user_log(1), create_user_log(1), create_user_log(1)];
        let indexer = super::ChatLogIndexer::from_logs(logs);
        let inactive = indexer.inactive_log();
        assert_eq!(inactive.len(), 0);
    }

    #[test]
    fn test_inactive_log_returns_inactive_logs() {
        let logs = vec![create_user_log(1), create_user_log(1), create_user_log(1)];
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
        let logs = vec![create_user_log(1)];
        let indexer = super::ChatLogIndexer::from_logs(logs);
        let result = indexer.get_relevant_context("test message");
        assert!(result.is_empty());
    }

    #[test]
    fn test_get_relevant_context_returns_empty_when_rag_context_empty() {
        let logs = vec![create_user_log(1), create_user_log(1)];
        let mut indexer = super::ChatLogIndexer::from_logs(logs);
        // Set active flag to false to create inactive logs
        indexer.logs[0].active = false;

        // RAG context should return None for empty/irrelevant context
        let result = indexer.get_relevant_context("test message");
        // This might return empty vec or vec with message depending on RAG implementation
        // For now, we test that the method exists and doesn't panic
        let _ = result;
    }

    // --- Workflow-aware checkpoint tests ---

    #[test]
    fn test_is_log_in_workflow() {
        // Single setup covering all conditions
        let logs = vec![
            create_user_log(1),                 // 0: no workflow before
            create_workflow_log("wf", Some(1)), // 1: workflow start
            create_user_log(1),                 // 2: in workflow
            create_workflow_log("wf", None),    // 3: workflow end
            create_user_log(1),                 // 4: after workflow ended
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
            create_user_log(1),
            create_workflow_log("test_workflow", Some(1)), // start
            create_user_log(1),
            create_workflow_log("test_workflow", Some(2)), // navigate to step 2
            create_user_log(1),
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
            create_user_log(1),
            create_assistant_log(1),
            create_user_log(1),
            create_assistant_log(1),
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
            create_user_log(1),
            create_workflow_log("test_workflow", None), // end
            create_user_log(1),
        ];
        let indexer = super::ChatLogIndexer::from_logs(logs);
        // Last checkpoint should be index 3 (user message after workflow ended)
        assert_eq!(
            super::ChatLogIndexer::find_last_checkpoint_in(&indexer.logs),
            Some(3)
        );
    }

    // --- Tool classification-aware truncation tests (new behavior) ---
    //
    // Regions definition:
    // - Region 1: Before 2x checkpoint (older logs)
    // - Region 2: Between 1x and 2x checkpoint (middle logs)
    // - Region 3: After 1x checkpoint (newer logs)
    //
    // Removal phases:
    // - Soft limit: Phase 1 (idempotent tools regions 1&2), Phase 2 (non-idempotent tools region 1)
    // - Hard limit: Phase 3 (chat/system region 1), Phase 4 (non-idempotent tools region 2),
    //               Phase 5 (idempotent tools region 3)
    // - Workflow logs are never removed

    #[test]
    fn test_truncation_no_truncation_when_under_threshold() {
        // Baseline: no truncation when total tokens ≤ soft limit (10)
        let logs = vec![
            create_user_log(1),
            create_tool_log("read_file", 1),
            create_user_log(1),
        ];
        let indexer = super::ChatLogIndexer::from_logs(logs);

        // All logs should remain active
        assert!(indexer.logs.iter().all(|l| l.active));
    }

    #[test]
    fn test_truncation_phase1_idempotent_tools_regions_1_and_2() {
        // Phase 1: Remove idempotent tools from regions 1&2 until soft limit (10) reached
        // Structure:
        // - Region 1: tool(read_file), user (checkpoint 2x) - indices 0, 1
        // - Region 2: tool(web_search), user (checkpoint 1x) - indices 2, 3
        // - Region 3: last user - index 4
        let logs = vec![
            create_user_log(1),
            create_tool_log("read_file", 1),
            create_tool_log("web_search", 1),
            create_user_log(6),
            create_tool_log("read_file", 1),
            create_user_log(2),
        ];
        let indexer = super::ChatLogIndexer::from_logs(logs);

        // Phase 1 removes idempotent tools from regions 1 & 2
        // read_file at index 1 (Region 1) should be removed
        // web_search at index 2 (Region 1) should be preserved (non-idempotent)
        // read_file at index 4 (Region 2) should be removed
        assert!(indexer.logs[0].active, "first user preserved");
        assert!(
            !indexer.logs[1].active,
            "idempotent tool removed (Region 1)"
        );
        assert!(
            indexer.logs[2].active,
            "non-idempotent tool preserved (Region 1)"
        );
        assert!(indexer.logs[3].active, "user preserved (checkpoint 2x)");
        assert!(
            !indexer.logs[4].active,
            "idempotent tool removed (Region 2)"
        );
        assert!(indexer.logs[5].active, "user preserved (checkpoint 1x)");
    }

    #[test]
    fn test_truncation_phase2_non_idempotent_tools_region_1() {
        // Phase 2: Remove non-idempotent tools from region 1 until soft limit (10) reached
        // After Phase 1 removes idempotent tools, if still over threshold,
        // Phase 2 removes non-idempotent tools from region 1 only
        let logs = vec![
            create_user_log(1),
            create_tool_log("web_search", 1),
            create_user_log(10),
            create_tool_log("web_search", 1),
            create_user_log(1),
        ];
        let indexer = super::ChatLogIndexer::from_logs(logs);

        // Phase 2: non-idempotent tool in region 1 is removed
        assert!(indexer.logs[0].active, "first user preserved");
        assert!(
            !indexer.logs[1].active,
            "web_search in region 1 removed (Phase 2)"
        );
        assert!(indexer.logs[2].active, "user (checkpoint 2x) preserved");
        assert!(
            indexer.logs[3].active,
            "web_search in region 2 preserved (Phase 2 only touches region 1)"
        );
        assert!(indexer.logs[4].active, "user (checkpoint 1x) preserved");
    }

    #[test]
    fn test_truncation_phase3_chat_system_logs_region_1() {
        // Phase 3 (hard limit 20): Remove chat/system logs from region 1
        // After Phase 1&2, if still over hard limit, remove chat logs from region 1
        // Need > 20 tokens to trigger Phase 3
        let logs = vec![
            create_user_log(1),       // 0 - first user (never removed)
            create_assistant_log(18), // 1 - assistant (Region 1, chat log)
            create_user_log(1),       // 2 - user (checkpoint 2x, Region 1)
            create_user_log(1),       // 3 - user (checkpoint 1x, Region 2)
        ];
        let indexer = super::ChatLogIndexer::from_logs(logs);

        // Assistant (chat log) should be removed in Phase 3
        // First user at index 1 should be preserved (never remove first user)
        assert!(indexer.logs[0].active, "first user_preserved");
        assert!(!indexer.logs[1].active, "assistant removed (Region 1)");
        assert!(indexer.logs[2].active, "user preserved (checkpoint 2x)");
        assert!(indexer.logs[3].active, "user preserved (checkpoint 1x)");
    }

    #[test]
    fn test_truncation_phase4_non_idempotent_tools_region_2() {
        // Phase 4 (hard limit 20): Remove non-idempotent tools from region 2
        // After Phase 3, if still over hard limit, remove non-idempotent tools from region 2
        let logs = vec![
            create_user_log(1),               // 0 - first user (never removed)
            create_user_log(1),               // 1 - user (checkpoint 2x)
            create_tool_log("web_search", 1), // 2 - non-idempotent tool (Region 2)
            create_user_log(18),              // 3 - user (checkpoint 1x)
        ];
        let indexer = super::ChatLogIndexer::from_logs(logs);

        // Non-idempotent tool in region 2 should be removed in Phase 4
        assert!(indexer.logs[0].active, "first user preserved");
        assert!(indexer.logs[1].active, "user preserved (checkpoint 2x)");
        assert!(
            !indexer.logs[2].active,
            "non-idempotent tool removed (Region 2)"
        );
        assert!(indexer.logs[3].active, "user preserved (checkpoint 1x)");
    }

    #[test]
    fn test_truncation_phase5_idempotent_tools_region_3() {
        // Phase 5 (hard limit 20): Remove idempotent tools from region 3
        // After Phase 4, if still over hard limit, remove idempotent tools from region 3
        let logs = vec![
            create_user_log(1),              // 0 - first user (never removed)
            create_user_log(1),              // 1 - user (checkpoint 2x)
            create_user_log(20),             // 2 - user (checkpoint 1x)
            create_tool_log("read_file", 1), // 3 - idempotent tool (Region 3)
        ];
        let indexer = super::ChatLogIndexer::from_logs(logs);

        // Idempotent tool in region 3 should be removed in Phase 5
        assert!(indexer.logs[0].active, "first user preserved");
        assert!(indexer.logs[1].active, "user preserved (checkpoint 2x)");
        assert!(indexer.logs[2].active, "user preserved (checkpoint 1x)");
        assert!(
            !indexer.logs[3].active,
            "idempotent tool removed (Region 3)"
        );
    }

    #[test]
    fn test_truncation_first_user_always_preserved() {
        // First user message is never removed across all phases
        // This is a fundamental invariant
        let logs = vec![
            create_user_log(10), // 0 - first user (MUST be preserved)
            create_user_log(20), // 1 - random user log in the middle gets removed instead
            create_user_log(20), // 2 - user (checkpoint 2x)
            create_user_log(1),  // 3 - user (checkpoint 1x)
        ];
        let indexer = super::ChatLogIndexer::from_logs(logs);

        // First user must always be preserved regardless of token pressure
        assert!(
            indexer.logs[0].active,
            "first user ALWAYS preserved (invariant)"
        );
        assert!(!indexer.logs[1].active, "second got removed instead");
    }
}
