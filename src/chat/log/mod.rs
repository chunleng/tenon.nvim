pub mod handler;
pub mod indexer;
pub mod window;

use chrono::{DateTime, TimeZone, Utc};
use rig::message::{AssistantContent, Image, Message, ToolResultContent, UserContent};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use skimtoken::estimate_tokens;

use crate::utils::format_yaml_block_scalars;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TenonUserMessage {
    Text(String),
}

impl From<&TenonUserMessage> for Message {
    fn from(value: &TenonUserMessage) -> Self {
        match value {
            TenonUserMessage::Text(msg) => Message::User {
                content: vec![UserContent::text(msg.clone())],
            },
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TenonAssistantMessageContent {
    Text(String),
}

impl From<&TenonAssistantMessageContent> for AssistantContent {
    fn from(value: &TenonAssistantMessageContent) -> Self {
        match value {
            TenonAssistantMessageContent::Text(s) => AssistantContent::text(s.clone()),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TenonAssistantMessage {
    pub reasoning: Option<String>,
    pub content: Vec<TenonAssistantMessageContent>,
}

impl From<&TenonAssistantMessage> for Option<Message> {
    fn from(value: &TenonAssistantMessage) -> Self {
        // reasoning is not return to consciously reduce context
        if value.content.is_empty() {
            return None;
        }
        Some(Message::Assistant {
            id: None,
            content: value.content.iter().map(Into::into).collect(),
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TenonToolCall {
    pub id: String,
    pub internal_call_id: String,
    pub name: String,
    pub args: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TenonToolResult {
    Text(rig::agent::Text),
    Image(Image),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TenonToolError(pub String);

impl TenonToolError {
    /// Strip rig's internal wrapping prefix for display.
    /// E.g. "ToolCallError: read_file ..." → "read_file ..."
    pub fn display_message(&self) -> &str {
        let mut s = self.0.as_str();
        while let Some(stripped) = s.strip_prefix("ToolCallError: ") {
            s = stripped;
        }
        s
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TenonToolLog {
    pub tool_call: TenonToolCall,
    pub tool_result: Option<Result<TenonToolResult, TenonToolError>>,
}

impl From<&TenonToolLog> for Vec<Message> {
    fn from(value: &TenonToolLog) -> Self {
        // Dual ids to fit the most troublesome OpenAI response API:
        // item handle `fc_...` + correlator `call_...`
        let item_id = format!("fc_{}", &value.tool_call.id);
        let call_id = format!("call_{}", &value.tool_call.id);
        let mut messages = vec![Message::Assistant {
            id: None,
            content: vec![AssistantContent::tool_call_with_call_id(
                item_id.clone(),
                call_id.clone(),
                value.tool_call.name.clone(),
                value.tool_call.args.clone(),
            )],
        }];
        if let Some(res) = &value.tool_result {
            let tool_result_content = match res {
                Ok(TenonToolResult::Text(text)) => vec![ToolResultContent::Text(text.clone())],
                Ok(TenonToolResult::Image(img)) => vec![ToolResultContent::Image(img.clone())],
                Err(err) => vec![ToolResultContent::text(&err.0)],
            };
            messages.push(Message::User {
                content: vec![UserContent::tool_result_with_call_id(
                    item_id,
                    call_id,
                    value.tool_call.name.clone(),
                    tool_result_content,
                )],
            });
        }

        messages
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TenonThoughtLog {
    pub thought: String,
    pub summary: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TenonChoreoLog {
    pub id: String,
    pub content: String,
    /// Move number within the choreo. `None` for end-of-choreo logs.
    /// Alias "step" keeps histories saved before the workflow→choreo rename loadable.
    #[serde(default, alias = "step")]
    pub r#move: Option<usize>,
    /// The tool log (call + result) that navigated to this choreo move.
    #[serde(default)]
    pub tool_log: TenonToolLog,
}

impl TenonChoreoLog {
    pub fn new(
        id: impl ToString,
        content: impl ToString,
        move_number: Option<usize>,
        tool_log: TenonToolLog,
    ) -> Self {
        Self {
            id: id.to_string(),
            content: content.to_string(),
            r#move: move_number,
            tool_log,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TenonLogData {
    User(TenonUserMessage),
    Assistant(TenonAssistantMessage),
    Tool(TenonToolLog),
    Thought(TenonThoughtLog),
    /// Alias "Workflow" keeps histories saved before the workflow→choreo rename loadable.
    #[serde(alias = "Workflow")]
    Choreo(TenonChoreoLog),
}

fn zero() -> usize {
    0
}

fn datetime_min() -> DateTime<Utc> {
    Utc.timestamp_nanos(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{TimeZone, Utc};

    #[test]
    fn test_last_updated_at_set_on_creation() {
        let before = Utc::now();
        let log = TenonLog::new(TenonLogData::User(TenonUserMessage::Text(
            "test".to_string(),
        )));
        let after = Utc::now();

        assert!(log.last_updated_at >= before);
        assert!(log.last_updated_at <= after);
    }

    #[test]
    fn test_last_updated_at_defaults_to_min_when_missing() {
        let json = r#"{"token_count":5,"User":{"Text":"hello"}}"#;
        let log: TenonLog = serde_json::from_str(json).unwrap();
        assert_eq!(log.last_updated_at, Utc.timestamp_nanos(0));
    }

    #[test]
    fn test_set_tool_result_updates_last_updated_at() {
        let mut log = TenonLog::new(TenonLogData::Tool(TenonToolLog {
            tool_call: TenonToolCall {
                id: "1".into(),
                internal_call_id: "1".into(),
                name: "test".into(),
                args: serde_json::json!({}),
            },
            tool_result: None,
        }));
        std::thread::sleep(std::time::Duration::from_millis(10));
        let before = Utc::now();
        log.set_tool_result(Some(Ok(TenonToolResult::Text(rig::agent::Text {
            text: "result".into(),
            ..Default::default()
        }))));
        let after = Utc::now();

        assert!(log.last_updated_at >= before);
        assert!(log.last_updated_at <= after);
    }

    #[test]
    fn test_append_reasoning_updates_last_updated_at() {
        let mut log = TenonLog::new(TenonLogData::Assistant(TenonAssistantMessage {
            reasoning: None,
            content: vec![],
        }));
        std::thread::sleep(std::time::Duration::from_millis(10));
        let before = Utc::now();
        log.append_reasoning("thinking");
        let after = Utc::now();

        assert!(log.last_updated_at >= before);
        assert!(log.last_updated_at <= after);
    }

    #[test]
    fn test_choreo_detail_lines_extracts_artifact_from_navigate_choreo() {
        let choreo_log = TenonChoreoLog {
            id: "c-1".to_string(),
            content: "Test Choreo".to_string(),
            r#move: Some(2),
            tool_log: TenonToolLog {
                tool_call: TenonToolCall {
                    id: "call-1".to_string(),
                    internal_call_id: "call-1".to_string(),
                    name: "navigate_choreo".to_string(),
                    args: serde_json::json!({"move": 2, "move_artifact": "scope analysis done"}),
                },
                tool_result: Some(Ok(TenonToolResult::Text(rig::agent::Text {
                    text: "output:\n  move: 2\n  artifact: scope analysis done".to_string(),
                    ..Default::default()
                }))),
            },
        };

        let log = TenonLog::new(TenonLogData::Choreo(choreo_log));
        let lines = log.data().detail_lines();

        let joined = lines.join("\n");
        assert!(
            joined.contains("### Artifact (Previous Move)"),
            "should have Artifact header, got: {joined}"
        );
        assert!(
            joined.contains("scope analysis done"),
            "should extract artifact value from YAML, got: {joined}"
        );
    }

    #[test]
    fn test_choreo_detail_lines_extracts_artifact_from_end_choreo() {
        let choreo_log = TenonChoreoLog {
            id: "c-1".to_string(),
            content: "Test Choreo".to_string(),
            r#move: None,
            tool_log: TenonToolLog {
                tool_call: TenonToolCall {
                    id: "call-1".to_string(),
                    internal_call_id: "call-1".to_string(),
                    name: "end_choreo".to_string(),
                    args: serde_json::json!({"move_artifact": "final summary of work"}),
                },
                tool_result: Some(Ok(TenonToolResult::Text(rig::agent::Text {
                    text: "choreo completed. output: final summary of work".to_string(),
                    ..Default::default()
                }))),
            },
        };

        let log = TenonLog::new(TenonLogData::Choreo(choreo_log));
        let lines = log.data().detail_lines();

        let joined = lines.join("\n");
        assert!(
            joined.contains("### Artifact (Final)"),
            "should have Artifact (Final) header, got: {joined}"
        );
        assert!(
            joined.contains("final summary of work"),
            "should extract move_artifact value from args, got: {joined}"
        );
    }

    #[test]
    fn test_choreo_log_deserializes_legacy_workflow_json() {
        // History files saved before the workflow→choreo rename use the
        // "Workflow" variant tag and "step" field name. They must keep loading.
        let json = r#"{"token_count":5,"Workflow":{"id":"wf-1","content":"Test Workflow","step":2,"tool_log":{"tool_call":{"id":"1","internal_call_id":"1","name":"navigate_workflow","args":{}},"tool_result":null}}}"#;
        let log: TenonLog = serde_json::from_str(json).unwrap();
        assert!(matches!(log.data(), TenonLogData::Choreo(c) if c.r#move == Some(2)));
    }

    #[test]
    fn test_choreo_log_deserializes_choreo_json() {
        let json = r#"{"token_count":5,"Choreo":{"id":"c-1","content":"Test Choreo","move":2,"tool_log":{"tool_call":{"id":"1","internal_call_id":"1","name":"navigate_choreo","args":{}},"tool_result":null}}}"#;
        let log: TenonLog = serde_json::from_str(json).unwrap();
        assert!(matches!(log.data(), TenonLogData::Choreo(c) if c.r#move == Some(2)));
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TenonLog {
    #[serde(default = "zero")]
    pub token_count: usize,
    #[serde(default = "datetime_min")]
    pub last_updated_at: DateTime<Utc>,
    #[serde(flatten)]
    pub data: TenonLogData,
}

impl TenonLog {
    pub fn new(data: TenonLogData) -> Self {
        let token_count = data.count_tokens();
        Self {
            data,
            token_count,
            last_updated_at: Utc::now(),
        }
    }

    pub fn data(&self) -> &TenonLogData {
        &self.data
    }

    /// Converts the log to a string for embedding.
    /// Returns None if the log type should not be indexed for RAG.
    pub fn to_embeddable_text(&self) -> Option<String> {
        match &self.data {
            TenonLogData::User(msg) => match msg {
                TenonUserMessage::Text(text) => Some(text.clone()),
            },
            TenonLogData::Assistant(msg) => Some(
                msg.content
                    .iter()
                    .map(|c| match c {
                        TenonAssistantMessageContent::Text(t) => t.clone(),
                    })
                    .collect::<Vec<_>>()
                    .join("\n"),
            ),
            TenonLogData::Tool(tool_log) => {
                let mut text = format!(
                    "Tool: {}\nArgs: {}",
                    tool_log.tool_call.name, tool_log.tool_call.args
                );
                if let Some(result) = &tool_log.tool_result {
                    match result {
                        Ok(TenonToolResult::Text(t)) => {
                            text.push_str(&format!("\nResult: {}", t.text));
                        }
                        Ok(TenonToolResult::Image(_)) => {
                            text.push_str("\nResult: [Image]");
                        }
                        Err(e) => {
                            text.push_str(&format!("\nError: {}", e.0));
                        }
                    }
                }
                Some(text)
            }
            TenonLogData::Thought(thought_log) => Some(thought_log.thought.clone()),
            TenonLogData::Choreo(_) => None,
        }
    }

    /// Updates the tool result and recalculates token count.
    /// Panics if this is not a Tool log.
    pub fn set_tool_result(&mut self, result: Option<Result<TenonToolResult, TenonToolError>>) {
        match &mut self.data {
            TenonLogData::Tool(tool_log) => {
                tool_log.tool_result = result;
                self.token_count = self.data.count_tokens();
                self.last_updated_at = Utc::now();
            }
            _ => panic!("set_tool_result called on non-Tool TenonLog"),
        }
    }

    /// Appends reasoning text. As reasoning is omitted from token count, there's no need to
    /// count_tokens
    /// Returns true if an existing Assistant message was updated, false if a new one was created.
    pub fn append_reasoning(&mut self, reasoning: &str) -> bool {
        match &mut self.data {
            TenonLogData::Assistant(msg) => {
                match &mut msg.reasoning {
                    Some(text) => text.push_str(reasoning),
                    None => msg.reasoning = Some(reasoning.to_string()),
                }
                self.last_updated_at = Utc::now();
                true
            }
            _ => false,
        }
    }

    /// Appends text content and recalculates token count.
    /// Returns true if an existing Assistant message was updated, false if a new one was created.
    pub fn append_text(&mut self, text: &str) -> bool {
        match &mut self.data {
            TenonLogData::Assistant(msg) => {
                if let Some(TenonAssistantMessageContent::Text(last_text)) = msg.content.last_mut()
                {
                    last_text.push_str(text);
                    self.token_count += estimate_tokens(text);
                } else {
                    msg.content
                        .push(TenonAssistantMessageContent::Text(text.to_string()));
                    self.token_count = self.data.count_tokens();
                }
                self.last_updated_at = Utc::now();
                true
            }
            _ => false,
        }
    }

    /// Returns the token count for this log entry.
    pub fn token_count(&self) -> usize {
        self.token_count
    }
}

impl TenonLogData {
    /// Returns true if this log is a system tool that should be hidden from the chat display.
    /// System tools with error results are shown so the user can see what went wrong.
    pub fn is_hidden_system_tool(&self) -> bool {
        match self {
            TenonLogData::Tool(tool_log) => {
                crate::tools::get_tool_classification(&tool_log.tool_call.name)
                    == crate::tools::ToolClassification::System
                    && !matches!(tool_log.tool_result, Some(Err(_)))
            }
            _ => false,
        }
    }

    /// Formats this log's content for detail display using level 3 markdown headers
    /// for categories and plain text for content.
    pub fn detail_lines(&self) -> Vec<String> {
        fn plain(text: &str) -> Vec<String> {
            text.lines().map(|l| l.to_string()).collect()
        }

        match self {
            TenonLogData::User(TenonUserMessage::Text(text)) => plain(text),
            TenonLogData::Assistant(msg) => {
                let mut lines = Vec::new();
                if let Some(reasoning) = &msg.reasoning {
                    lines.push("### Reasoning".to_string());
                    lines.push(String::new());
                    lines.extend(plain(reasoning));
                    lines.push(String::new());
                }
                lines.push("### Text".to_string());
                lines.push(String::new());
                for content in &msg.content {
                    match content {
                        TenonAssistantMessageContent::Text(text) => lines.extend(plain(text)),
                    }
                }
                lines
            }
            TenonLogData::Tool(log) => {
                let mut lines = vec![
                    "### Tool".to_string(),
                    String::new(),
                    log.tool_call.name.clone(),
                ];
                lines.push(String::new());
                lines.push("### Args".to_string());
                lines.push(String::new());
                let args_yaml = serde_yaml::to_string(&log.tool_call.args)
                    .unwrap_or_else(|_| log.tool_call.args.to_string());
                lines.extend(plain(&format_yaml_block_scalars(&args_yaml)));
                lines.push(String::new());
                match &log.tool_result {
                    None => {
                        lines.push("### Result".to_string());
                        lines.push(String::new());
                        lines.push("(pending)".to_string());
                    }
                    Some(Ok(TenonToolResult::Text(text))) => {
                        lines.push("### Result".to_string());
                        lines.push(String::new());
                        lines.extend(plain(&text.text));
                    }
                    Some(Ok(TenonToolResult::Image(_))) => {
                        lines.push("### Result".to_string());
                        lines.push(String::new());
                        lines.push("[Image]".to_string());
                    }
                    Some(Err(err)) => {
                        lines.push("### Error".to_string());
                        lines.push(String::new());
                        lines.extend(plain(&err.0));
                    }
                }
                lines
            }
            TenonLogData::Thought(log) => {
                let mut lines = vec!["### Thought".to_string(), String::new()];
                lines.extend(plain(&log.thought));
                if let Some(summary) = &log.summary {
                    lines.push(String::new());
                    lines.push("### Summary".to_string());
                    lines.push(String::new());
                    lines.extend(plain(summary));
                }
                lines
            }
            TenonLogData::Choreo(log) => {
                let move_display = log
                    .r#move
                    .map(|m| m.to_string())
                    .unwrap_or_else(|| "(end)".to_string());
                let mut lines = vec![
                    "### ID".to_string(),
                    String::new(),
                    log.id.clone(),
                    String::new(),
                    "### Move".to_string(),
                    String::new(),
                    move_display,
                    String::new(),
                ];
                lines.push("### Choreo Title".to_string());
                lines.push(String::new());
                lines.extend(plain(&log.content));
                if log.tool_log.tool_call.name == "navigate_choreo" {
                    match &log.tool_log.tool_result {
                        Some(Ok(TenonToolResult::Text(text))) => {
                            let output_text = serde_yaml::from_str::<serde_yaml::Value>(&text.text)
                                .ok()
                                .and_then(|parsed| {
                                    parsed
                                        .get("artifact")
                                        .and_then(|v| v.as_str())
                                        .map(String::from)
                                })
                                .unwrap_or_else(|| text.text.clone());
                            lines.push(String::new());
                            lines.push("### Artifact (Previous Move)".to_string());
                            lines.push(String::new());
                            lines.extend(plain(&output_text));
                        }
                        _ => {}
                    }
                } else if log.tool_log.tool_call.name == "end_choreo" {
                    // end_choreo carries its artifact in the call args, not the result
                    let artifact = log
                        .tool_log
                        .tool_call
                        .args
                        .get("move_artifact")
                        .and_then(|v| v.as_str())
                        .unwrap_or("(none)");
                    lines.push(String::new());
                    lines.push("### Artifact (Final)".to_string());
                    lines.push(String::new());
                    lines.extend(plain(artifact));
                }
                lines
            }
        }
    }

    fn count_tokens(&self) -> usize {
        match self {
            TenonLogData::User(msg) => match msg {
                TenonUserMessage::Text(text) => estimate_tokens(text),
            },
            TenonLogData::Assistant(msg) => {
                // Reasoning is not counted because it's not used for sending request

                msg.content
                    .iter()
                    .map(|c| match c {
                        TenonAssistantMessageContent::Text(text) => estimate_tokens(text),
                    })
                    .sum::<usize>()
            }
            TenonLogData::Tool(log) => {
                let call_tokens = estimate_tokens(&log.tool_call.name)
                    + estimate_tokens(&log.tool_call.args.to_string());
                let result_tokens = match &log.tool_result {
                    None => 0,
                    Some(Ok(res)) => match res {
                        TenonToolResult::Text(text) => estimate_tokens(&text.text),
                        TenonToolResult::Image(_) => 0, // Images don't have simple token count
                    },
                    Some(Err(err)) => estimate_tokens(&err.0),
                };
                call_tokens + result_tokens
            }
            TenonLogData::Thought(log) => estimate_tokens(&log.thought),
            TenonLogData::Choreo(_) => 0,
        }
    }
}

impl From<&TenonLog> for Vec<Message> {
    fn from(value: &TenonLog) -> Self {
        match &value.data {
            TenonLogData::User(user_message) => vec![user_message.into()],
            TenonLogData::Assistant(assistant_message) => {
                match Option::<Message>::from(assistant_message) {
                    Some(x) => vec![x],
                    None => vec![],
                }
            }
            TenonLogData::Tool(tool_log) => tool_log.into(),
            TenonLogData::Thought(thought_log) => {
                vec![Message::Assistant {
                    id: None,
                    content: vec![AssistantContent::text(format!(
                        "Thoughts: {}",
                        thought_log.thought
                    ))],
                }]
            }
            TenonLogData::Choreo(_) => vec![],
        }
    }
}
