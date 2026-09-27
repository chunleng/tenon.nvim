use crate::agent::worker::simple::SimpleTenonWorkerAgent;
use crate::chat::log::TenonThoughtLog;
use crate::chat::{TenonLog, TenonLogData};
use crate::tools::{ToolCore, ToolCoreCall};
use rig::tool::{ToolContext, ToolExecutionError, ToolOutput};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::sync::{Arc, RwLock};

/// Result text returned to the model and stored in the log. The summary is
/// deliberately excluded: it lives in the Thought log struct instead.
const RESULT_TEXT: &str = "Thought recorded";

#[derive(Deserialize)]
pub struct RecordThoughtArgs {
    /// Required-arg validation happens at deserialization; the value itself is
    /// read from the log args in `convert_log`.
    #[allow(dead_code)]
    pub thought: String,
}

#[derive(Deserialize, Serialize, Clone)]
pub struct RecordThought;

impl ToolCore for RecordThought {
    fn name(&self) -> String {
        "record_thought".to_string()
    }
    type Error = ToolExecutionError;
    type Args = RecordThoughtArgs;
    type Output = String;
    type Call = RecordThoughtCall;

    fn description(&self) -> String {
        "Use when performing complex reasoning or some cache memory is needed".to_string()
    }

    fn parameters(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "thought": {
                    "type": "string",
                    "description": "A thought to think about"
                }
            },
            "required": ["thought"]
        })
    }

    async fn init_call(
        &self,
        _context: &mut ToolContext,
        _args: Self::Args,
    ) -> Result<Self::Call, Self::Error> {
        Ok(RecordThoughtCall)
    }

    async fn convert_log(&self, log: Arc<RwLock<TenonLog>>) -> Option<ToolOutput> {
        // Read the thought from the log args; the lock is dropped before the
        // async summarization.
        let thought = {
            let log = log.read().ok()?;
            match &log.data {
                TenonLogData::Tool(tool_log) => tool_log
                    .tool_call
                    .args
                    .get("thought")
                    .and_then(|v| v.as_str())
                    .map(String::from)?,
                _ => return None,
            }
        };
        let summary = if thought.len() < 100 {
            None
        } else {
            summarize_thought(&thought).await
        };
        let mut log = log.write().ok()?;
        let TenonLogData::Tool(tool_log) = &log.data else {
            return None;
        };
        let tool_log = tool_log.clone();
        log.data = TenonLogData::Thought(TenonThoughtLog { summary, tool_log });
        log.refresh();

        None
    }
}

pub struct RecordThoughtCall;

impl ToolCoreCall for RecordThoughtCall {
    type Output = String;
    type Error = ToolExecutionError;

    async fn result(self, _context: &mut ToolContext) -> Result<Self::Output, Self::Error> {
        Ok(RESULT_TEXT.to_string())
    }
}

async fn summarize_thought(thought: &str) -> Option<String> {
    let worker = SimpleTenonWorkerAgent::new(
        None,
        "Summarize the following thought into 1 to 3 top-level bullet points. \
         Use '-' for bullet points. \
         Output must be shorter than original message \
         Output only the bullet points, nothing else.",
        Some(serde_json::Map::new()),
    )
    .ok()?;

    worker
        .chat(format!("Thought to summarize:\n```\n{}\n```", thought))
        .await
        .ok()
        .filter(|s| !s.trim().is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rig::tool::ToolContext;
    use std::sync::{Arc, RwLock};

    use crate::chat::log::{TenonLog, TenonLogData, TenonToolCall, TenonToolLog};

    fn thought_tool_log(args: serde_json::Value) -> TenonToolLog {
        TenonToolLog {
            tool_call: TenonToolCall {
                id: "call-1".to_string(),
                internal_call_id: "call-1".to_string(),
                item_id: None,
                name: "record_thought".to_string(),
                args,
            },
            tool_result: None,
            progress: vec![],
        }
    }

    #[tokio::test]
    async fn test_result_is_confirmation_without_summary() {
        let output = RecordThoughtCall
            .result(&mut ToolContext::new())
            .await
            .unwrap();
        assert_eq!(output, RESULT_TEXT);
    }

    #[tokio::test]
    async fn test_convert_log_short_thought_skips_summarization() {
        let short_thought = "This is a short thought under 100 chars.";
        let log = Arc::new(RwLock::new(TenonLog::new(TenonLogData::Tool(
            thought_tool_log(serde_json::json!({"thought": short_thought})),
        ))));

        // None keeps the original output; the conversion is silent
        assert!(RecordThought.convert_log(log.clone()).await.is_none());

        let log = log.read().unwrap();
        let TenonLogData::Thought(thought_log) = log.data() else {
            panic!("expected Thought log after convert_log");
        };
        assert_eq!(thought_log.summary, None);
        assert_eq!(thought_log.thought(), short_thought);
    }

    #[tokio::test]
    async fn test_convert_log_recalculates_token_count() {
        let thought = "a thought that is long enough to have several tokens in it";
        let log = Arc::new(RwLock::new(TenonLog::new(TenonLogData::Tool(
            thought_tool_log(serde_json::json!({"thought": thought})),
        ))));

        RecordThought.convert_log(log.clone()).await;

        let log = log.read().unwrap();
        // Token count reflects the thought text only, not the embedded tool log
        assert_eq!(log.token_count, crate::utils::estimate_tokens(thought));
    }

    #[tokio::test]
    async fn test_convert_log_non_tool_log_returns_none() {
        let log = Arc::new(RwLock::new(TenonLog::new(TenonLogData::Thought(
            TenonThoughtLog {
                summary: None,
                tool_log: thought_tool_log(serde_json::json!({"thought": "t"})),
            },
        ))));

        assert!(RecordThought.convert_log(log.clone()).await.is_none());

        // Log unchanged
        assert!(matches!(
            log.read().unwrap().data(),
            TenonLogData::Thought(_)
        ));
    }
}
