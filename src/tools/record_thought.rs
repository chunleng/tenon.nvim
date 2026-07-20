use rig::tool::{Tool, ToolError};
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
    type Error = ToolError;
    type Args = RecordThoughtArgs;
    type Output = String;

    fn description(&self) -> String {
        "Use the tool to think about something. It will not obtain new information or change the \
         database, but just append the thought to the log. Use it when complex reasoning or some \
         cache memory is needed."
            .to_string()
    }

    fn parameters(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "thought": {
                    "type": "string",
                    "description": "A thought to think about."
                }
            },
            "required": ["thought"]
        })
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        Ok(args.thought)
    }
}
