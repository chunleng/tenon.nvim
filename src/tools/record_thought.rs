use crate::agent::worker::simple::SimpleTenonWorkerAgent;
use rig::tool::{Tool, ToolContext, ToolExecutionError};
use serde::{Deserialize, Serialize};
use serde_json::json;

#[derive(Deserialize)]
pub struct RecordThoughtArgs {
    pub thought: String,
}

#[derive(Deserialize, Serialize, Clone)]
pub struct RecordThought;

impl Tool for RecordThought {
    const NAME: &'static str = "record_thought";
    type Error = ToolExecutionError;
    type Args = RecordThoughtArgs;
    type Output = String;

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

    async fn call(
        &self,
        _context: &mut ToolContext,
        args: Self::Args,
    ) -> Result<Self::Output, Self::Error> {
        let summary = if args.thought.len() < 100 {
            None
        } else {
            summarize_thought(&args.thought).await
        };

        Ok(json!({
            "thought": args.thought,
            "summary": summary,
        })
        .to_string())
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
    use rig::tool::{Tool, ToolContext};

    #[tokio::test]
    async fn test_short_thought_skips_summarization() {
        let tool = RecordThought;
        let short_thought = "This is a short thought under 100 chars.";
        let output = tool
            .call(
                &mut ToolContext::new(),
                RecordThoughtArgs {
                    thought: short_thought.to_string(),
                },
            )
            .await
            .unwrap();

        let parsed: serde_json::Value = serde_json::from_str(&output).unwrap();
        assert_eq!(parsed["thought"], short_thought);
        assert!(parsed["summary"].is_null());
    }
}
