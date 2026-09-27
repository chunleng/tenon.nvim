pub mod handler;
pub mod indexer;
pub mod window;

use chrono::{DateTime, TimeZone, Utc};
use rig::message::{AssistantContent, Image, Message, ToolResultContent, UserContent};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::utils::{estimate_tokens, format_yaml_block_scalars};

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

impl TenonAssistantMessage {
    /// Chat counts as empty when missing or whitespace-only, so reasoning can be shown instead.
    pub fn chat_is_empty(&self) -> bool {
        self.content.iter().all(|c| match c {
            TenonAssistantMessageContent::Text(s) => s.trim().is_empty(),
        })
    }
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
    /// Provider-issued output-item id (OpenAI Responses `fc_...`), if any.
    #[serde(default)]
    pub item_id: Option<String>,
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

fn serialize_progress<S: serde::Serializer>(
    lines: &[String],
    serializer: S,
) -> Result<S::Ok, S::Error> {
    serializer.serialize_str(&lines.join("\n"))
}

fn deserialize_progress<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Vec<String>, D::Error> {
    let joined = String::deserialize(deserializer)?;
    Ok(joined.lines().map(str::to_string).collect())
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TenonToolLog {
    pub tool_call: TenonToolCall,
    pub tool_result: Option<Result<TenonToolResult, TenonToolError>>,
    /// Streaming progress lines collected while the tool is still running.
    /// Persisted as a single string joined by newlines to keep history compact.
    #[serde(
        default,
        serialize_with = "serialize_progress",
        deserialize_with = "deserialize_progress"
    )]
    pub progress: Vec<String>,
}

impl From<&TenonToolLog> for Vec<Message> {
    fn from(value: &TenonToolLog) -> Self {
        // Dual ids to fit the most troublesome OpenAI response API:
        // item handle `fc_...` + correlator `call_...`
        let item_id = value
            .tool_call
            .item_id
            .clone()
            .unwrap_or_else(|| format!("fc_{}", value.tool_call.id));
        let call_id = format!("call_{}", value.tool_call.id);
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
    pub summary: Option<String>,
    /// The original `record_thought` tool log
    pub tool_log: TenonToolLog,
}

impl TenonThoughtLog {
    /// The thought text, read from the embedded tool call args.
    pub fn thought(&self) -> String {
        self.tool_log
            .tool_call
            .args
            .get("thought")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TenonChoreoLog {
    pub id: String,
    pub content: String,
    /// Move number within the choreo. `None` for end-of-choreo logs.
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

    /// Extracts the artifact passed from the previous step, if any.
    /// `navigate_choreo` carries it in the result YAML; `end_choreo` in the call args.
    fn previous_step_artifact(&self) -> Option<String> {
        match self.tool_log.tool_call.name.as_str() {
            "navigate_choreo" => {
                if let Some(Ok(TenonToolResult::Text(text))) = &self.tool_log.tool_result {
                    serde_yaml::from_str::<serde_yaml::Value>(&text.text)
                        .ok()
                        .and_then(|parsed| {
                            parsed
                                .get("artifact")
                                .and_then(|v| v.as_str())
                                .map(String::from)
                        })
                } else {
                    None
                }
            }
            "end_choreo" => self
                .tool_log
                .tool_call
                .args
                .get("move_artifact")
                .and_then(|v| v.as_str())
                .map(String::from),
            _ => None,
        }
    }

    /// Builds the `<context type="choreo">` system content replayed into LLM history.
    pub fn system_content(&self) -> String {
        let header = match self.tool_log.tool_call.name.as_str() {
            "use_choreo" => format!("Choreo started: \"{}\"", self.content),
            "navigate_choreo" => format!("Choreo move changed: \"{}\"", self.content),
            _ => "The choreo has ended".to_string(),
        };
        let mut content = format!("<context type=\"choreo\">\n{header}\n");
        if let Some(artifact) = self.previous_step_artifact()
            && artifact.is_empty()
        {
            content.push_str(&format!(
                "The following information has been passed from the previous step:\n\
                 ```yaml\n\
                 {artifact}\n\
                 ```\n"
            ));
        }
        content.push_str("</context>\n");
        content
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TenonLogData {
    User(TenonUserMessage),
    Assistant(TenonAssistantMessage),
    Tool(TenonToolLog),
    Thought(TenonThoughtLog),
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
                item_id: None,
                name: "test".into(),
                args: serde_json::json!({}),
            },
            tool_result: None,
            progress: vec![],
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
    fn test_tool_progress_persisted_as_joined_string() {
        let log = TenonLog::new(TenonLogData::Tool(TenonToolLog {
            tool_call: TenonToolCall {
                id: "1".into(),
                internal_call_id: "1".into(),
                item_id: None,
                name: "test".into(),
                args: serde_json::json!({}),
            },
            tool_result: None,
            progress: vec!["step 1".to_string(), "step 2".to_string()],
        }));

        let json = serde_json::to_string(&log).unwrap();
        assert!(
            json.contains(r#""progress":"step 1\nstep 2""#),
            "progress must be persisted as a single joined string, got: {json}"
        );

        let round_tripped: TenonLog = serde_json::from_str(&json).unwrap();
        let TenonLogData::Tool(tool_log) = round_tripped.data() else {
            panic!("expected tool log")
        };
        assert_eq!(
            tool_log.progress,
            vec!["step 1".to_string(), "step 2".to_string()],
            "deserialized progress should split back into lines"
        );
    }

    #[test]
    fn test_tool_progress_empty_round_trips_to_empty() {
        let log = TenonLog::new(TenonLogData::Tool(TenonToolLog {
            tool_call: TenonToolCall {
                id: "1".into(),
                internal_call_id: "1".into(),
                item_id: None,
                name: "test".into(),
                args: serde_json::json!({}),
            },
            tool_result: None,
            progress: vec![],
        }));

        let json = serde_json::to_string(&log).unwrap();
        let round_tripped: TenonLog = serde_json::from_str(&json).unwrap();
        let TenonLogData::Tool(tool_log) = round_tripped.data() else {
            panic!("expected tool log")
        };
        assert!(
            tool_log.progress.is_empty(),
            "empty progress should round-trip to empty vec"
        );
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
                    item_id: None,
                    name: "navigate_choreo".to_string(),
                    args: serde_json::json!({"move": 2, "move_artifact": "scope analysis done"}),
                },
                tool_result: Some(Ok(TenonToolResult::Text(rig::agent::Text {
                    text: "output:\n  move: 2\n  artifact: scope analysis done".to_string(),
                    ..Default::default()
                }))),
                progress: vec![],
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
                    item_id: None,
                    name: "end_choreo".to_string(),
                    args: serde_json::json!({"move_artifact": "final summary of work"}),
                },
                tool_result: Some(Ok(TenonToolResult::Text(rig::agent::Text {
                    text: "choreo completed. output: final summary of work".to_string(),
                    ..Default::default()
                }))),
                progress: vec![],
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

    fn choreo_tool_log(name: &str, args: serde_json::Value, result_text: &str) -> TenonToolLog {
        TenonToolLog {
            tool_call: TenonToolCall {
                id: "call-1".to_string(),
                internal_call_id: "call-1".to_string(),
                item_id: None,
                name: name.to_string(),
                args,
            },
            tool_result: Some(Ok(TenonToolResult::Text(rig::agent::Text {
                text: result_text.to_string(),
                ..Default::default()
            }))),
            progress: vec![],
        }
    }

    #[test]
    fn test_errored_choreo_tool_returns_messages() {
        // Errored choreo results enter LLM history like any other tool error.
        // Successful choreo ToolLogs are transient: TenonTool::call converts them
        // into Choreo logs before the engine ever sees them.
        for name in ["use_choreo", "navigate_choreo", "end_choreo"] {
            let mut tool_log = choreo_tool_log(name, serde_json::json!({}), "");
            tool_log.tool_result = Some(Err(TenonToolError(
                "ToolCallError: Invalid navigation from move 1 to move 5".to_string(),
            )));
            let log = TenonLog::new(TenonLogData::Tool(tool_log));
            let messages = Vec::<Message>::from(&log);
            assert!(
                !messages.is_empty(),
                "errored {name} should emit history messages"
            );
        }
    }

    #[test]
    fn test_errored_choreo_tool_counts_tokens() {
        let mut tool_log = choreo_tool_log("navigate_choreo", serde_json::json!({"move": 5}), "");
        tool_log.tool_result = Some(Err(TenonToolError(
            "ToolCallError: Invalid navigation from move 1 to move 5".to_string(),
        )));
        let log = TenonLog::new(TenonLogData::Tool(tool_log));
        assert!(
            log.token_count > 0,
            "errored choreo tool log should count tokens like any tool log"
        );
    }

    #[test]
    fn test_other_tools_still_return_messages() {
        let log = TenonLog::new(TenonLogData::Tool(choreo_tool_log(
            "read_file",
            serde_json::json!({"filepath": "./src/lib.rs"}),
            "file contents",
        )));
        let messages = Vec::<Message>::from(&log);
        assert!(
            !messages.is_empty(),
            "read_file should emit history messages"
        );
    }

    #[test]
    fn test_refresh_recalculates_token_count_and_timestamp() {
        let mut log = TenonLog::new(TenonLogData::Tool(choreo_tool_log(
            "read_file",
            serde_json::json!({"filepath": "./src/lib.rs"}),
            "",
        )));
        let before = log.last_updated_at;
        if let TenonLogData::Tool(tool_log) = &mut log.data {
            tool_log.tool_result = Some(Ok(TenonToolResult::Text(rig::agent::Text {
                text: "file contents".to_string(),
                ..Default::default()
            })));
        }
        log.refresh();

        let expected = TenonLog::new(TenonLogData::Tool(choreo_tool_log(
            "read_file",
            serde_json::json!({"filepath": "./src/lib.rs"}),
            "file contents",
        )));
        assert_eq!(log.token_count, expected.token_count);
        assert_ne!(log.last_updated_at, before);
    }

    #[test]
    fn test_thought_helper_falls_back_to_empty() {
        let thought_log = TenonThoughtLog {
            summary: None,
            tool_log: choreo_tool_log("record_thought", serde_json::json!({}), ""),
        };
        assert_eq!(thought_log.thought(), "");
    }

    #[test]
    fn test_choreo_log_converts_to_system_context() {
        let choreo_log = TenonChoreoLog {
            id: "c-1".to_string(),
            content: "Test Choreo".to_string(),
            r#move: Some(2),
            tool_log: choreo_tool_log(
                "navigate_choreo",
                serde_json::json!({"move": 2}),
                "artifact: scope analysis done",
            ),
        };
        let log = TenonLog::new(TenonLogData::Choreo(choreo_log));
        let messages = Vec::<Message>::from(&log);

        assert_eq!(messages.len(), 1, "expected single system message");
        let _ = match &messages[0] {
            Message::System { content } => content,
            other => panic!("expected System message, got {other:?}"),
        };
    }

    #[test]
    fn test_choreo_log_deserializes_choreo_json() {
        let json = r#"{"token_count":5,"Choreo":{"id":"c-1","content":"Test Choreo","move":2,"tool_log":{"tool_call":{"id":"1","internal_call_id":"1","name":"navigate_choreo","args":{}},"tool_result":null}}}"#;
        let log: TenonLog = serde_json::from_str(json).unwrap();
        assert!(matches!(log.data(), TenonLogData::Choreo(c) if c.r#move == Some(2)));
    }

    #[test]
    fn test_to_embeddable_text() {
        // User text
        let log = TenonLog::new(TenonLogData::User(TenonUserMessage::Text(
            "hello".to_string(),
        )));
        assert_eq!(log.to_embeddable_text(), "hello");

        // Assistant text
        let log = TenonLog::new(TenonLogData::Assistant(TenonAssistantMessage {
            reasoning: None,
            content: vec![TenonAssistantMessageContent::Text("reply".to_string())],
        }));
        assert_eq!(log.to_embeddable_text(), "reply");

        // Thought
        let log = TenonLog::new(TenonLogData::Thought(TenonThoughtLog {
            summary: None,
            tool_log: choreo_tool_log(
                "record_thought",
                serde_json::json!({"thought": "thinking"}),
                "",
            ),
        }));
        assert_eq!(log.to_embeddable_text(), "thinking");

        // Choreo returns its id
        let log = TenonLog::new(TenonLogData::Choreo(TenonChoreoLog::new(
            "c-1",
            "Test Choreo",
            Some(2),
            TenonToolLog::default(),
        )));
        assert_eq!(log.to_embeddable_text(), "c-1");
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
    pub fn to_embeddable_text(&self) -> String {
        match &self.data {
            TenonLogData::User(msg) => match msg {
                TenonUserMessage::Text(text) => text.clone(),
            },
            TenonLogData::Assistant(msg) => msg
                .content
                .iter()
                .map(|c| match c {
                    TenonAssistantMessageContent::Text(t) => t.clone(),
                })
                .collect::<Vec<_>>()
                .join("\n"),
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
                text
            }
            TenonLogData::Thought(thought_log) => thought_log.thought(),
            TenonLogData::Choreo(choreo_log) => choreo_log.id.clone(),
        }
    }

    /// Recalculates the token count and updates last_updated_at.
    pub fn refresh(&mut self) {
        self.token_count = self.data.count_tokens();
        self.last_updated_at = Utc::now();
    }

    /// Updates the tool result and recalculates token count.
    /// Panics if this is not a Tool log.
    pub fn set_tool_result(&mut self, result: Option<Result<TenonToolResult, TenonToolError>>) {
        match &mut self.data {
            TenonLogData::Tool(tool_log) => tool_log.tool_result = result,
            _ => panic!("set_tool_result called on non-Tool TenonLog"),
        }
        self.refresh();
    }

    /// Appends streaming progress text to a Tool log. As in-progress text is
    /// display-only (not part of LLM history), there's no need to count_tokens.
    /// No-op if this is not a Tool log.
    pub fn append_tool_progress(&mut self, text: &str) {
        if let TenonLogData::Tool(tool_log) = &mut self.data {
            tool_log.progress.extend(text.lines().map(str::to_string));
            self.last_updated_at = Utc::now();
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
    /// Returns the role string for this log, used in the `<chat-history role="...">` tag.
    pub fn role(&self) -> &'static str {
        match self {
            TenonLogData::User(_) => "user",
            TenonLogData::Assistant(_) => "assistant",
            TenonLogData::Tool(_) => "tool",
            TenonLogData::Thought(_) => "thought",
            TenonLogData::Choreo(_) => "choreo",
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
                if !log.progress.is_empty() {
                    lines.push("### Progress".to_string());
                    lines.push(String::new());
                    lines.extend(log.progress.iter().cloned());
                    lines.push(String::new());
                }
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
                lines.extend(plain(&log.thought()));
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
                    if let Some(Ok(TenonToolResult::Text(text))) = &log.tool_log.tool_result {
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
            TenonLogData::Thought(log) => estimate_tokens(&log.thought()),
            TenonLogData::Choreo(log) => estimate_tokens(&log.system_content()),
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
                        thought_log.thought()
                    ))],
                }]
            }
            TenonLogData::Choreo(choreo_log) => {
                vec![Message::System {
                    content: choreo_log.system_content(),
                }]
            }
        }
    }
}
