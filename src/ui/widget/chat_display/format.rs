pub trait DisplayChatFormatter {
    fn lines(&self) -> Vec<String>;
    fn line_hl_group(&self) -> String;
    fn sign(&self) -> String;
    fn sign_hl_group(&self) -> String;
}

impl DisplayChatFormatter for crate::chat::TenonLogData {
    fn lines(&self) -> Vec<String> {
        use crate::chat::{TenonAssistantMessageContent, TenonLogData, TenonUserMessage};
        match self {
            TenonLogData::User(msg) => match msg {
                TenonUserMessage::Text(text_msg) => {
                    text_msg.0.lines().map(|s| s.to_string()).collect()
                }
            },
            TenonLogData::Assistant(msg) => {
                // If content exists, show content; otherwise show reasoning
                if msg.content.is_empty() {
                    msg.reasoning
                        .as_ref()
                        .map(|r| r.lines().map(|s| s.to_string()).collect())
                        .unwrap_or_default()
                } else {
                    msg.content
                        .iter()
                        .flat_map(|c| match c {
                            TenonAssistantMessageContent::Text(text) => {
                                text.lines().map(|s| s.to_string()).collect::<Vec<_>>()
                            }
                        })
                        .collect()
                }
            }
            TenonLogData::Tool(log) => {
                let prefix = match &log.tool_result {
                    None => " ",
                    Some(Ok(_)) => " ",
                    Some(Err(_)) => " ",
                };
                let summary =
                    crate::tools::tool_display_summary(&log.tool_call.name, &log.tool_call.args);
                let first_line = match summary {
                    Some(s) => format!("{} {} | {}", prefix, log.tool_call.name, s),
                    None => format!("{} {}", prefix, log.tool_call.name),
                };
                let mut lines = vec![first_line];
                if let Some(Err(err)) = &log.tool_result {
                    lines.push(format!("   > {}", err.display_message()));
                }
                lines
            }
            TenonLogData::Workflow(wf) => {
                vec![format!("# {}", wf.content)]
            }
        }
    }

    fn line_hl_group(&self) -> String {
        use crate::chat::TenonLogData;
        match self {
            TenonLogData::User(_) => String::new(),
            TenonLogData::Assistant(msg) => {
                if msg.content.is_empty() {
                    "TenonLineAssistantReasoning".to_string()
                } else {
                    String::new()
                }
            }
            TenonLogData::Tool(_) => "TenonLineTool".to_string(),
            TenonLogData::Workflow(_) => String::new(),
        }
    }

    fn sign(&self) -> String {
        use crate::chat::TenonLogData;
        match self {
            TenonLogData::User(_) => " ".to_string(),
            TenonLogData::Assistant(msg) => {
                if msg.content.is_empty() {
                    " ".to_string()
                } else {
                    "󰚩 ".to_string()
                }
            }
            TenonLogData::Tool(_) => "󰣖 ".to_string(),
            TenonLogData::Workflow(_) => " ".to_string(),
        }
    }

    fn sign_hl_group(&self) -> String {
        use crate::chat::TenonLogData;
        match self {
            TenonLogData::User(_) => "TenonSignUser".to_string(),
            TenonLogData::Assistant(msg) => {
                if msg.content.is_empty() {
                    "TenonSignAssistantReasoning".to_string()
                } else {
                    "TenonSignAssistantTalk".to_string()
                }
            }
            TenonLogData::Tool(_) => "TenonSignTool".to_string(),
            TenonLogData::Workflow(_) => "TenonSignWorkflow".to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chat::{
        TenonAssistantMessage, TenonAssistantMessageContent, TenonLogData, TenonToolCall,
        TenonToolError, TenonToolLog, TenonToolResult, TenonUserMessage, TenonUserTextMessage,
        TenonWorkflowLog,
    };
    use serde_json::json;

    #[test]
    fn test_user_formatter() {
        let msg = TenonUserMessage::Text(TenonUserTextMessage("Hello\nWorld".to_string()));
        let data = TenonLogData::User(msg);

        assert_eq!(data.lines(), vec!["Hello", "World"]);
        assert_eq!(data.sign(), " ");
        assert_eq!(data.sign_hl_group(), "TenonSignUser");
        assert_eq!(data.line_hl_group(), "");
    }

    #[test]
    fn test_assistant_reasoning_formatter() {
        let msg = TenonAssistantMessage {
            reasoning: Some("thinking...".to_string()),
            content: vec![],
        };
        let data = TenonLogData::Assistant(msg);

        assert_eq!(data.lines(), vec!["thinking..."]);
        assert_eq!(data.sign(), " ");
        assert_eq!(data.sign_hl_group(), "TenonSignAssistantReasoning");
        assert_eq!(data.line_hl_group(), "TenonLineAssistantReasoning");
    }

    #[test]
    fn test_assistant_talk_formatter() {
        let msg = TenonAssistantMessage {
            reasoning: None,
            content: vec![TenonAssistantMessageContent::Text(
                "Hello\nWorld".to_string(),
            )],
        };
        let data = TenonLogData::Assistant(msg);

        assert_eq!(data.lines(), vec!["Hello", "World"]);
        assert_eq!(data.sign(), "󰚩 ");
        assert_eq!(data.sign_hl_group(), "TenonSignAssistantTalk");
        assert_eq!(data.line_hl_group(), "");
    }

    #[test]
    fn test_assistant_with_reasoning_and_content() {
        let msg = TenonAssistantMessage {
            reasoning: Some("internal thoughts".to_string()),
            content: vec![TenonAssistantMessageContent::Text(
                "Hello\nWorld".to_string(),
            )],
        };
        let data = TenonLogData::Assistant(msg);

        // Should show content only, not reasoning
        assert_eq!(data.lines(), vec!["Hello", "World"]);
        assert_eq!(data.sign(), "󰚩 ");
        assert_eq!(data.sign_hl_group(), "TenonSignAssistantTalk");
        assert_eq!(data.line_hl_group(), "");
    }

    #[test]
    fn test_tool_pending_formatter() {
        let tool_call = TenonToolCall {
            id: "1".to_string(),
            internal_call_id: "call_1".to_string(),
            name: "read_file".to_string(),
            args: json!({"path": "test.txt"}),
        };
        let log = TenonToolLog {
            tool_call,
            tool_result: None,
        };
        let data = TenonLogData::Tool(log);

        assert_eq!(data.lines(), vec!["  read_file"]);
        assert_eq!(data.sign(), "󰣖 ");
        assert_eq!(data.sign_hl_group(), "TenonSignTool");
        assert_eq!(data.line_hl_group(), "TenonLineTool");
    }

    #[test]
    fn test_tool_success_formatter() {
        let tool_call = TenonToolCall {
            id: "1".to_string(),
            internal_call_id: "call_1".to_string(),
            name: "read_file".to_string(),
            args: json!({"path": "test.txt"}),
        };
        let log = TenonToolLog {
            tool_call,
            tool_result: Some(Ok(TenonToolResult::Text(rig::agent::Text {
                text: "content".to_string(),
            }))),
        };
        let data = TenonLogData::Tool(log);

        assert_eq!(data.lines(), vec!["  read_file"]);
        assert_eq!(data.sign(), "󰣖 ");
        assert_eq!(data.sign_hl_group(), "TenonSignTool");
        assert_eq!(data.line_hl_group(), "TenonLineTool");
    }

    #[test]
    fn test_tool_error_formatter() {
        let tool_call = TenonToolCall {
            id: "1".to_string(),
            internal_call_id: "call_1".to_string(),
            name: "read_file".to_string(),
            args: json!({"path": "test.txt"}),
        };
        let log = TenonToolLog {
            tool_call,
            tool_result: Some(Err(TenonToolError("File not found".to_string()))),
        };
        let data = TenonLogData::Tool(log);

        assert_eq!(data.lines(), vec!["  read_file", "   > File not found"]);
        assert_eq!(data.sign(), "󰣖 ");
        assert_eq!(data.sign_hl_group(), "TenonSignTool");
        assert_eq!(data.line_hl_group(), "TenonLineTool");
    }

    #[test]
    fn test_workflow_formatter() {
        let workflow = TenonWorkflowLog::new("wf_1", "Processing step 1", Some(1));
        let data = TenonLogData::Workflow(workflow);

        assert_eq!(data.lines(), vec!["# Processing step 1"]);
        assert_eq!(data.sign(), " ");
        assert_eq!(data.sign_hl_group(), "TenonSignWorkflow");
        assert_eq!(data.line_hl_group(), "");
    }
}
