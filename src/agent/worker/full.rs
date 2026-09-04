use std::sync::atomic::AtomicBool;
use std::sync::{Arc, RwLock};

use rig::tool::{DynamicTool, Tool, ToolContext, ToolExecutionError};
use serde::Deserialize;
use serde_json::json;

use crate::agent::engine::{AgenticAgentType, AgenticStreamEngine};
use crate::tools::into_dynamic_tool;
use crate::{chat::choreo::Choreo, clients::SupportedModels, directive::Directive};

#[derive(Debug, Clone)]
pub struct TenonAgent {
    pub model: SupportedModels,
    pub directive: Vec<Directive>,
    pub tool_names: Vec<String>,
    pub choreos: Vec<Arc<Choreo>>,
}

impl TenonAgent {
    pub fn new(
        model: SupportedModels,
        directive: Vec<Directive>,
        tools: &[impl AsRef<str>],
        choreos: Vec<Arc<Choreo>>,
    ) -> Self {
        Self {
            model,
            directive,
            tool_names: tools.iter().map(|t| t.as_ref().to_string()).collect(),
            choreos,
        }
    }
}

/// Result of a goal-oriented task execution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GoalResult {
    /// The agent found and submitted an answer.
    Answer(String),
    /// The agent could not find an answer. Contains an explanation if provided.
    NoAnswer(Option<String>),
}

#[derive(Deserialize)]
struct AnswerToolArgs {
    prompt: String,
    is_answer: bool,
}

/// Tool that lets the goal-oriented agent signal task completion.
/// Writes the result to shared state checked by the agent's loop after each round.
#[derive(Clone)]
struct AnswerTool {
    result: Arc<RwLock<Option<GoalResult>>>,
}

impl Tool for AnswerTool {
    const NAME: &'static str = "submit_answer";
    type Error = ToolExecutionError;
    type Args = AnswerToolArgs;
    type Output = String;

    fn description(&self) -> String {
        "Submit your final answer or indicate no answer was found. \
         Call this when you have completed the task."
            .to_string()
    }

    fn parameters(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "prompt": {
                    "type": "string",
                    "description": "The answer or explanation"
                },
                "is_answer": {
                    "type": "boolean",
                    "description": "true if this is the answer, false if no answer was found"
                }
            },
            "required": ["prompt", "is_answer"]
        })
    }

    async fn call(
        &self,
        _context: &mut ToolContext,
        args: Self::Args,
    ) -> Result<Self::Output, Self::Error> {
        let result = if args.is_answer {
            GoalResult::Answer(args.prompt.clone())
        } else if !args.prompt.is_empty() {
            GoalResult::NoAnswer(Some(args.prompt))
        } else {
            GoalResult::NoAnswer(None)
        };

        if let Ok(mut guard) = self.result.write() {
            *guard = Some(result);
        }

        Ok(json!({
            "submitted": true,
            "is_answer": args.is_answer,
        })
        .to_string())
    }
}

/// Autonomous agent that pursues a goal until it calls the answer tool.
/// Uses AgenticStreamEngine for history management (RAG, context truncation).
pub struct GoalOrientedWorker {
    engine: AgenticStreamEngine,
    result_slot: Arc<RwLock<Option<GoalResult>>>,
    rounds: usize,
    overtime_rounds: usize,
    max_turns: usize,
}

impl GoalOrientedWorker {
    pub fn new(model: SupportedModels, directive: Vec<Directive>, tools: Vec<DynamicTool>) -> Self {
        let result_slot: Arc<RwLock<Option<GoalResult>>> = Arc::new(RwLock::new(None));

        let answer_tool = into_dynamic_tool(AnswerTool {
            result: Arc::clone(&result_slot),
        });

        let mut all_tools = vec![answer_tool];
        all_tools.extend(tools);

        let engine =
            AgenticStreamEngine::new(model, directive, all_tools, vec![], AgenticAgentType::Tool);

        Self {
            engine,
            result_slot,
            rounds: 5,
            overtime_rounds: 3,
            max_turns: 5,
        }
    }

    #[allow(unused)]
    pub fn rounds(mut self, rounds: usize) -> Self {
        self.rounds = rounds;
        self
    }

    #[allow(unused)]
    pub fn overtime_rounds(mut self, overtime_rounds: usize) -> Self {
        self.overtime_rounds = overtime_rounds;
        self
    }

    #[allow(unused)]
    pub fn max_turns(mut self, max_turns: usize) -> Self {
        self.max_turns = max_turns;
        self
    }

    /// Runs the task through normal rounds then overtime rounds until the
    /// answer tool is called or all rounds are exhausted.
    pub async fn perform_task(&mut self, task: &str) -> GoalResult {
        if let Ok(mut guard) = self.result_slot.write() {
            *guard = None;
        }

        let task_with_instruction = format!(
            "{task}\n\n\
             Once you have the answer, submit it using the `submit_answer` tool \
             with `is_answer` set to true."
        );

        let cancel_token = AtomicBool::new(false);

        for _ in 0..self.rounds {
            self.engine
                .process_turn(
                    task_with_instruction.clone(),
                    &cancel_token,
                    |_| {},
                    self.max_turns,
                )
                .await;

            if let Some(result) = self.result_slot.read().ok().and_then(|g| g.clone()) {
                return result;
            }
        }

        let overtime_prompt = format!(
            "{task_with_instruction}\n\n\
             Complete the task now with what you found so far. \
             Reply with the answer using the answer tool, or state that there is no answer."
        );

        for _ in 0..self.overtime_rounds {
            self.engine
                .process_turn(
                    overtime_prompt.clone(),
                    &cancel_token,
                    |_| {},
                    self.max_turns,
                )
                .await;

            if let Some(result) = self.result_slot.read().ok().and_then(|g| g.clone()) {
                return result;
            }
        }

        GoalResult::NoAnswer(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_answer_tool_is_answer_true() {
        let result_slot = Arc::new(RwLock::new(None));
        let tool = AnswerTool {
            result: Arc::clone(&result_slot),
        };

        tool.call(
            &mut ToolContext::new(),
            AnswerToolArgs {
                prompt: "The answer is 42".to_string(),
                is_answer: true,
            },
        )
        .await
        .unwrap();

        let result = result_slot.read().unwrap().clone().unwrap();
        assert_eq!(result, GoalResult::Answer("The answer is 42".to_string()));
    }

    #[tokio::test]
    async fn test_answer_tool_is_answer_false_with_prompt() {
        let result_slot = Arc::new(RwLock::new(None));
        let tool = AnswerTool {
            result: Arc::clone(&result_slot),
        };

        tool.call(
            &mut ToolContext::new(),
            AnswerToolArgs {
                prompt: "Could not find the answer".to_string(),
                is_answer: false,
            },
        )
        .await
        .unwrap();

        let result = result_slot.read().unwrap().clone().unwrap();
        assert_eq!(
            result,
            GoalResult::NoAnswer(Some("Could not find the answer".to_string()))
        );
    }

    #[tokio::test]
    async fn test_answer_tool_is_answer_false_empty_prompt() {
        let result_slot = Arc::new(RwLock::new(None));
        let tool = AnswerTool {
            result: Arc::clone(&result_slot),
        };

        tool.call(
            &mut ToolContext::new(),
            AnswerToolArgs {
                prompt: "".to_string(),
                is_answer: false,
            },
        )
        .await
        .unwrap();

        let result = result_slot.read().unwrap().clone().unwrap();
        assert_eq!(result, GoalResult::NoAnswer(None));
    }
}
