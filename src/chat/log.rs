use rig::{
    OneOrMany,
    message::{AssistantContent, Image, Message, ToolResult, ToolResultContent, UserContent},
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use skimtoken::estimate_tokens;

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

#[derive(Debug, Clone, Serialize, Deserialize)]
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

#[derive(Debug, Clone, Serialize, Deserialize)]
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
}

impl TenonWorkflowLog {
    pub fn new(id: impl ToString, content: impl ToString, step: Option<usize>) -> Self {
        Self {
            id: id.to_string(),
            content: content.to_string(),
            step,
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TenonLog {
    #[serde(default = "zero")]
    pub token_count: usize,
    #[serde(flatten)]
    pub data: TenonLogData,
}

impl TenonLog {
    pub fn new(data: TenonLogData) -> Self {
        let token_count = data.count_tokens();
        Self { data, token_count }
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
