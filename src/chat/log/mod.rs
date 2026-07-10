pub mod handler;
pub mod indexer;
pub mod window;

use chrono::{DateTime, TimeZone, Utc};
use rig::{
    OneOrMany,
    message::{AssistantContent, Image, Message, ToolResult, ToolResultContent, UserContent},
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use skimtoken::estimate_tokens;

use crate::utils::format_yaml_block_scalars;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TenonUserTextMessage(pub String);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TenonUserMessage {
    Text(TenonUserTextMessage),
}

impl From<TenonUserMessage> for Message {
    fn from(value: TenonUserMessage) -> Self {
        match value {
            TenonUserMessage::Text(TenonUserTextMessage(msg)) => Message::User {
                content: OneOrMany::one(UserContent::text(msg)),
            },
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TenonAssistantMessageContent {
    Text(String),
}

impl From<TenonAssistantMessageContent> for AssistantContent {
    fn from(value: TenonAssistantMessageContent) -> Self {
        match value {
            TenonAssistantMessageContent::Text(s) => AssistantContent::text(s),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TenonAssistantMessage {
    pub reasoning: Option<String>,
    pub content: Vec<TenonAssistantMessageContent>,
}

impl From<TenonAssistantMessage> for Option<Message> {
    fn from(value: TenonAssistantMessage) -> Self {
        // reasoning is not return to consciously reduce context
        Some(Message::Assistant {
            id: None,
            content: OneOrMany::many(value.content.into_iter().map(|x| x.into())).ok()?,
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
    /// Strip rig's internal wrapping prefixes for display.
    /// E.g. "Toolset error: ToolCallError: ToolCallError: read_file ..."
    ///   → "read_file ..."
    pub fn display_message(&self) -> &str {
        let mut s = self.0.strip_prefix("Toolset error: ").unwrap_or(&self.0);
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

impl From<TenonToolLog> for Vec<Message> {
    fn from(value: TenonToolLog) -> Self {
        let mut messages = vec![Message::Assistant {
            id: None,
            content: OneOrMany::one(AssistantContent::tool_call(
                value.tool_call.id.clone(),
                value.tool_call.name,
                value.tool_call.args,
            )),
        }];
        if let Some(res) = value.tool_result {
            let tool_result_content = match &res {
                Ok(TenonToolResult::Text(text)) => {
                    OneOrMany::one(ToolResultContent::Text(text.clone()))
                }
                Ok(TenonToolResult::Image(img)) => {
                    OneOrMany::one(ToolResultContent::Image(img.clone()))
                }
                Err(err) => OneOrMany::one(ToolResultContent::text(&err.0)),
            };
            messages.push(Message::User {
                content: OneOrMany::one(UserContent::ToolResult(ToolResult {
                    id: value.tool_call.id,
                    call_id: None,
                    content: tool_result_content,
                })),
            });
        }

        messages
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TenonWorkflowLog {
    pub id: String,
    pub content: String,
    /// Step number within the workflow. `None` for end-of-workflow logs.
    #[serde(default)]
    pub step: Option<usize>,
    /// The tool log (call + result) that navigated to this workflow step.
    #[serde(default)]
    pub tool_log: TenonToolLog,
}

impl TenonWorkflowLog {
    pub fn new(
        id: impl ToString,
        content: impl ToString,
        step: Option<usize>,
        tool_log: TenonToolLog,
    ) -> Self {
        Self {
            id: id.to_string(),
            content: content.to_string(),
            step,
            tool_log,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TenonLogData {
    User(TenonUserMessage),
    Assistant(TenonAssistantMessage),
    Tool(TenonToolLog),
    Workflow(TenonWorkflowLog),
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
            TenonUserTextMessage("test".to_string()),
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
    fn test_append_text_updates_last_updated_at() {
        let mut log = TenonLog::new(TenonLogData::Assistant(TenonAssistantMessage {
            reasoning: None,
            content: vec![],
        }));
        std::thread::sleep(std::time::Duration::from_millis(10));
        let before = Utc::now();
        log.append_text("hello");
        let after = Utc::now();

        assert!(log.last_updated_at >= before);
        assert!(log.last_updated_at <= after);
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
                TenonUserMessage::Text(TenonUserTextMessage(text)) => Some(text.clone()),
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
            TenonLogData::Workflow(_) => None,
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
            TenonLogData::User(TenonUserMessage::Text(TenonUserTextMessage(text))) => plain(text),
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
            TenonLogData::Workflow(log) => {
                let step = log
                    .step
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| "(end)".to_string());
                let mut lines = vec![
                    "### ID".to_string(),
                    String::new(),
                    log.id.clone(),
                    String::new(),
                    "### Step".to_string(),
                    String::new(),
                    step,
                    String::new(),
                ];
                lines.push("### Workflow Title".to_string());
                lines.push(String::new());
                lines.extend(plain(&log.content));
                if log.tool_log.tool_call.name == "navigate_workflow" {
                    match &log.tool_log.tool_result {
                        Some(Ok(TenonToolResult::Text(text))) => {
                            let output_text = serde_yaml::from_str::<serde_yaml::Value>(&text.text)
                                .ok()
                                .and_then(|parsed| {
                                    parsed
                                        .get("output")
                                        .and_then(|v| v.as_str())
                                        .map(String::from)
                                })
                                .unwrap_or_else(|| text.text.clone());
                            lines.push(String::new());
                            lines.push("### Output (Previous Step)".to_string());
                            lines.push(String::new());
                            lines.extend(plain(&output_text));
                        }
                        _ => {}
                    }
                }
                lines
            }
        }
    }

    fn count_tokens(&self) -> usize {
        match self {
            TenonLogData::User(msg) => match msg {
                TenonUserMessage::Text(TenonUserTextMessage(text)) => estimate_tokens(text),
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
            TenonLogData::Workflow(_) => 0,
        }
    }
}

impl From<TenonLog> for Vec<Message> {
    fn from(value: TenonLog) -> Self {
        match value.data {
            TenonLogData::User(user_message) => vec![user_message.into()],
            TenonLogData::Assistant(assistant_message) => {
                match Option::<Message>::from(assistant_message) {
                    Some(x) => vec![x],
                    None => vec![],
                }
            }
            TenonLogData::Tool(tool_log) => tool_log.into(),
            TenonLogData::Workflow(_) => vec![],
        }
    }
}
