use std::sync::Arc;

use crate::rag::RagContext;
use crate::tools::{ToolClassification, get_tool_classification};
use rig::completion::Message;

use super::window::LogWindow;
use super::{TenonLog, TenonLogData};

/// Wrapper around TenonLog for indexing purposes.
#[derive(Clone)]
pub struct IndexedLog {
    pub log: Arc<TenonLog>,
    pub active: bool,
}

/// Manages chat logs with context truncation and RAG support.
/// Encapsulates resume position tracking and RAG context management.
pub struct ChatLogIndexer {
    pub rag_context: RagContext,
}

impl ChatLogIndexer {
    #[cfg(not(test))]
    const MAX_ACTIVE_CONTEXT_TOKENS: usize = 10_000;

    #[cfg(test)]
    const MAX_ACTIVE_CONTEXT_TOKENS: usize = 10;

    #[cfg(not(test))]
    const HARD_LIMIT_ACTIVE_CONTEXT_TOKENS: usize = 30_000;

    #[cfg(test)]
    const HARD_LIMIT_ACTIVE_CONTEXT_TOKENS: usize = 20;

    /// Creates a new empty ChatLogIndexer.
    pub fn new() -> Self {
        Self {
            rag_context: RagContext::new(),
        }
    }

    /// Builds the chat history for an LLM request:
    /// applies context truncation, collects active messages, and prepends RAG context.
    pub fn retrieve_chatlog_with_context(
        &self,
        log_window: LogWindow,
        user_message: &str,
    ) -> Vec<Message> {
        let mut chat_history = log_window
            .active_history_log()
            .iter()
            .flat_map(|indexed| Vec::<Message>::from(TenonLog::clone(indexed)))
            .collect::<Vec<_>>();
        let history_messages = self.get_relevant_context(&log_window, user_message);
        for msg in history_messages.into_iter().rev() {
            chat_history.insert(0, msg);
        }
        chat_history
    }

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
    pub fn apply_context_truncation(&self, log_window: &mut LogWindow) {
        // Early return if under threshold
        let total = log_window.active_context_token_count();
        if total <= Self::MAX_ACTIVE_CONTEXT_TOKENS {
            return;
        }

        // Find checkpoints for region boundaries
        // Region 1: before second checkpoint (older logs)
        // Region 2: between first and second checkpoint (middle logs)
        // Region 3: after first checkpoint (newer logs)
        let first_checkpoint = log_window.find_last_checkpoint(None);
        let second_checkpoint =
            first_checkpoint.and_then(|fc| log_window.find_last_checkpoint(Some(fc)));

        // Find the first user message index (must never be removed)
        let first_user_idx = log_window.find_first_user_index();

        // Helper to check if index is first user
        let is_first_user = |idx: usize| first_user_idx == Some(idx);

        let tool_class = |log: &TenonLog| match log.data() {
            TenonLogData::Tool(tool_log) => Some(get_tool_classification(&tool_log.tool_call.name)),
            _ => None,
        };
        let is_idempotent_tool =
            |log: &TenonLog| tool_class(log) == Some(ToolClassification::Idempotent);
        let is_non_idempotent_tool = |log: &TenonLog| {
            matches!(
                tool_class(log),
                Some(
                    ToolClassification::NonMutating
                        | ToolClassification::Mutating
                        | ToolClassification::Unknown
                )
            )
        };
        let is_system_tool = |log: &TenonLog| tool_class(log) == Some(ToolClassification::System);

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
            let indexed = &log_window.logs[idx];
            if indexed.active && is_idempotent_tool(&indexed.log) {
                total_tokens -= indexed.log.token_count();
                log_window.logs[idx].active = false;
            }
        }

        // Region 2 idempotent tools
        for idx in region1_end..region2_end {
            if total_tokens <= Self::MAX_ACTIVE_CONTEXT_TOKENS {
                break;
            }
            let indexed = &log_window.logs[idx];
            if indexed.active && is_idempotent_tool(&indexed.log) {
                total_tokens -= indexed.log.token_count();
                log_window.logs[idx].active = false;
            }
        }

        // === SOFT LIMIT: Phase 2 - Remove non-idempotent tools from region 1 ===
        for idx in 0..region1_end {
            if total_tokens <= Self::MAX_ACTIVE_CONTEXT_TOKENS {
                break;
            }
            let indexed = &log_window.logs[idx];
            if indexed.active && is_non_idempotent_tool(&indexed.log) {
                total_tokens -= indexed.log.token_count();
                log_window.logs[idx].active = false;
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
                let indexed = &log_window.logs[idx];
                if indexed.active
                    && matches!(
                        &indexed.log.data(),
                        TenonLogData::User(_)
                            | TenonLogData::Assistant(_)
                            | TenonLogData::Thought(_)
                    )
                    && !is_first_user(idx)
                {
                    remaining_to_remove =
                        remaining_to_remove.saturating_sub(indexed.log.token_count());
                    log_window.logs[idx].active = false;
                }
            }

            // Remove system logs from region 1
            for idx in 0..region1_end {
                if remaining_to_remove == 0 {
                    break;
                }
                let indexed = &log_window.logs[idx];
                if indexed.active && is_system_tool(&indexed.log) {
                    remaining_to_remove =
                        remaining_to_remove.saturating_sub(indexed.log.token_count());
                    log_window.logs[idx].active = false;
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
                let indexed = &log_window.logs[idx];
                if indexed.active && is_non_idempotent_tool(&indexed.log) {
                    remaining_to_remove =
                        remaining_to_remove.saturating_sub(indexed.log.token_count());
                    log_window.logs[idx].active = false;
                }
            }
        }

        // === HARD LIMIT: Phase 5 - Remove idempotent tools from region 3 ===
        if total_tokens > Self::HARD_LIMIT_ACTIVE_CONTEXT_TOKENS {
            let mut remaining_to_remove = total_tokens - Self::HARD_LIMIT_ACTIVE_CONTEXT_TOKENS;

            for idx in region2_end..log_window.logs.len() {
                if remaining_to_remove == 0 {
                    break;
                }
                let indexed = &log_window.logs[idx];
                if indexed.active && is_idempotent_tool(&indexed.log) {
                    remaining_to_remove =
                        remaining_to_remove.saturating_sub(indexed.log.token_count());
                    log_window.logs[idx].active = false;
                }
            }
        }
    }

    /// Builds a history message from RAG context using inactive logs.
    /// Returns empty Vec if no user message provided, no inactive logs, or no relevant context found.
    /// Returns a Vec with one Message::User with <chat-history> wrapped context when found.
    fn get_relevant_context(&self, log_window: &LogWindow, user_message: &str) -> Vec<Message> {
        if user_message.is_empty() {
            return Vec::new();
        }
        // TODO we might want to produce 3 history log instead of one in the future
        let inactive_logs = log_window.inactive_log();
        self.rag_context
            .build_context(&inactive_logs, user_message)
            .map(|ctx| Message::System {
                content: format!("<chat-history>{}</chat-history>", ctx.trim()),
            })
            .into_iter()
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::LogWindow;
    use crate::chat::log::handler::ChatLogHandler;
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
                rig::agent::Text {
                    text: "ok".into(),
                    ..Default::default()
                },
            ))),
        }));
        log.token_count = token_count;
        log
    }

    #[test]
    fn test_active_context_token_count_empty() {
        let log_window = LogWindow { logs: Vec::new() };
        assert_eq!(log_window.active_context_token_count(), 0);
    }

    #[test]
    fn test_active_context_token_count_with_logs() {
        let logs = vec![create_user_log(5), create_user_log(5)];
        let handler = ChatLogHandler::from_logs(logs);
        let log_window = handler.log_window.read().unwrap();
        assert_eq!(log_window.active_context_token_count(), 10);
    }

    #[test]
    fn test_get_relevant_context_returns_empty_when_user_message_is_none() {
        let indexer = super::ChatLogIndexer::new();
        let log_window = LogWindow { logs: Vec::new() };
        let result = indexer.get_relevant_context(&log_window, "");
        assert!(result.is_empty());
    }

    #[test]
    fn test_get_relevant_context_returns_empty_when_no_inactive_logs() {
        let logs = vec![create_user_log(1)];
        let handler = ChatLogHandler::from_logs(logs);
        let indexer = handler.indexer.read().unwrap();
        let log_window = handler.log_window.read().unwrap();
        let result = indexer.get_relevant_context(&log_window, "test message");
        assert!(result.is_empty());
    }

    #[test]
    fn test_get_relevant_context_returns_empty_when_rag_context_empty() {
        let logs = vec![create_user_log(1), create_user_log(1)];
        let handler = ChatLogHandler::from_logs(logs);
        {
            let mut log_window = handler.log_window.write().unwrap();
            log_window.logs[0].active = false;
        }

        let indexer = handler.indexer.read().unwrap();
        let log_window = handler.log_window.read().unwrap();
        let result = indexer.get_relevant_context(&log_window, "test message");
        let _ = result;
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
        let handler = ChatLogHandler::from_logs(logs);
        let log_window = handler.log_window.read().unwrap();

        // All logs should remain active
        assert!(log_window.logs.iter().all(|l| l.active));
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
            create_assistant_log(0),
        ];
        let handler = ChatLogHandler::from_logs(logs);
        let log_window = handler.log_window.read().unwrap();

        // Phase 1 removes idempotent tools from regions 1 & 2
        // read_file at index 1 (Region 1) should be removed
        // web_search at index 2 (Region 1) should be preserved (non-idempotent)
        // read_file at index 4 (Region 2) should be removed
        assert!(log_window.logs[0].active, "first user preserved");
        assert!(
            !log_window.logs[1].active,
            "idempotent tool removed (Region 1)"
        );
        assert!(
            log_window.logs[2].active,
            "non-idempotent tool preserved (Region 1)"
        );
        assert!(log_window.logs[3].active, "user preserved (checkpoint 2x)");
        assert!(
            !log_window.logs[4].active,
            "idempotent tool removed (Region 2)"
        );
        assert!(log_window.logs[5].active, "user preserved (checkpoint 1x)");
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
            create_assistant_log(0),
        ];
        let handler = ChatLogHandler::from_logs(logs);
        let log_window = handler.log_window.read().unwrap();

        // Phase 2: non-idempotent tool in region 1 is removed
        assert!(log_window.logs[0].active, "first user preserved");
        assert!(
            !log_window.logs[1].active,
            "web_search in region 1 removed (Phase 2)"
        );
        assert!(log_window.logs[2].active, "user (checkpoint 2x) preserved");
        assert!(
            log_window.logs[3].active,
            "web_search in region 2 preserved (Phase 2 only touches region 1)"
        );
        assert!(log_window.logs[4].active, "user (checkpoint 1x) preserved");
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
            create_assistant_log(0),  // 4 - assistant (preserves last-user exclusion semantics)
        ];
        let handler = ChatLogHandler::from_logs(logs);
        let log_window = handler.log_window.read().unwrap();

        // Assistant (chat log) should be removed in Phase 3
        // First user at index 1 should be preserved (never remove first user)
        assert!(log_window.logs[0].active, "first user_preserved");
        assert!(!log_window.logs[1].active, "assistant removed (Region 1)");
        assert!(log_window.logs[2].active, "user preserved (checkpoint 2x)");
        assert!(log_window.logs[3].active, "user preserved (checkpoint 1x)");
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
            create_assistant_log(0), // 4 - assistant (preserves last-user exclusion semantics)
        ];
        let handler = ChatLogHandler::from_logs(logs);
        let log_window = handler.log_window.read().unwrap();

        // Non-idempotent tool in region 2 should be removed in Phase 4
        assert!(log_window.logs[0].active, "first user preserved");
        assert!(log_window.logs[1].active, "user preserved (checkpoint 2x)");
        assert!(
            !log_window.logs[2].active,
            "non-idempotent tool removed (Region 2)"
        );
        assert!(log_window.logs[3].active, "user preserved (checkpoint 1x)");
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
        let handler = ChatLogHandler::from_logs(logs);
        let log_window = handler.log_window.read().unwrap();

        // Idempotent tool in region 3 should be removed in Phase 5
        assert!(log_window.logs[0].active, "first user preserved");
        assert!(log_window.logs[1].active, "user preserved (checkpoint 2x)");
        assert!(log_window.logs[2].active, "user preserved (checkpoint 1x)");
        assert!(
            !log_window.logs[3].active,
            "idempotent tool removed (Region 3)"
        );
    }

    #[test]
    fn test_truncation_first_user_always_preserved() {
        // First user message is never removed across all phases
        // This is a fundamental invariant
        let logs = vec![
            create_user_log(10),     // 0 - first user (MUST be preserved)
            create_user_log(20),     // 1 - random user log in the middle gets removed instead
            create_user_log(20),     // 2 - user (checkpoint 2x)
            create_user_log(1),      // 3 - user (checkpoint 1x)
            create_assistant_log(0), // 4 - assistant (preserves last-user exclusion semantics)
        ];
        let handler = ChatLogHandler::from_logs(logs);
        let log_window = handler.log_window.read().unwrap();

        // First user must always be preserved regardless of token pressure
        assert!(
            log_window.logs[0].active,
            "first user ALWAYS preserved (invariant)"
        );
        assert!(!log_window.logs[1].active, "second got removed instead");
    }
}
