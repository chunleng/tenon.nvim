use anyhow::{Result, anyhow};
use serde::Deserialize;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use crate::chat::TenonWorkflowLog;
use crate::utils::plugin_path;

pub static WORKFLOW_BASE: OnceLock<std::path::PathBuf> = OnceLock::new();

pub fn workflow_path(relative: impl AsRef<Path>) -> std::path::PathBuf {
    WORKFLOW_BASE
        .get_or_init(|| plugin_path(std::path::PathBuf::from("markdown/workflow")))
        .join(relative)
}

/// Instruction content for a workflow step.
/// Can be inline text or a reference to a file.
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum Instruction {
    /// Inline text instruction
    Text(String),
    /// Path to a file containing the instruction
    File { file: PathBuf },
}

impl Instruction {
    /// Resolve the instruction to its final string content.
    ///
    /// For `Text`, returns the value directly.
    /// For `File`, reads the file contents. Relative paths are resolved
    /// against the workflow base directory.
    pub fn resolve(&self) -> Result<String> {
        match self {
            Instruction::Text(text) => Ok(text.clone()),
            Instruction::File { file } => std::fs::read_to_string(&file).map_err(|e| {
                anyhow!(
                    "Failed to read instruction file '{}': {}",
                    file.display(),
                    e
                )
            }),
        }
    }
}

impl Default for Instruction {
    fn default() -> Self {
        Instruction::Text(String::new())
    }
}

pub fn load_system_workflow() -> Workflow {
    Workflow {
        id: "ask_and_answer".to_string(),
        title: "Ask and Answer".to_string(),
        steps: vec![
            WorkflowStep {
                title: "Generate Question".to_string(),
                instruction: Instruction::Text(
                    "Generate a single multiple-choice question with exactly 4 options (A, B, C, D). \
                Example:\n\
                Question: <question text>\n\
                A) <option A>\n\
                B) <option B>\n\
                C) <option C>\n\
                D) <option D>\n\
                No explanation, no extra questions. Output question to chat"
                        .to_string(),
                ),
                goto_instructions: vec![
                    WorkflowGotoInstruction {
                        to: GotoStep::Next,
                        condition: None,
                        output: "the question and options".to_string(),
                    }
                ],
            },
            WorkflowStep {
                title: "Random Answer".to_string(),
                instruction: Instruction::Text(
                    "- Fetch https://www.random.org/integers/?num=1&min=0&max=999&col=1&base=10&format=plain, a random number will be generated. Do not reuse this number.\n\
                    - 0–249 → A\n\
                    - 250–499 → B\n\
                    - 500–749 → C\n\
                    - 750–999 → D
                    - Output choice to chat"
                        .to_string(),
                ),
                goto_instructions: vec![
                    WorkflowGotoInstruction {
                        to: GotoStep::Next,
                        condition: None,
                        output: "chosen answer".to_string(),
                    }
                ],
            },
            WorkflowStep {
                title: "Evaluate Answer".to_string(),
                instruction: Instruction::Text(
                    "You are an evaluator. Check whether the random answer from the previous step \
                is correct for the generated question. Output correct or wrong with explanation. Congratulate the user if correct"
                        .to_string(),
                ),
                goto_instructions: vec![WorkflowGotoInstruction{
                    to: GotoStep::Step(2),
                    condition: Some("wrong answer".to_string()),
                    output: "reason why answer is wrong. never reveal the correct answer".to_string(),
                }],
            },
        ],
        default_condition: "user is bored".to_string(),
    }
}

#[derive(Clone, Deserialize)]
pub struct Workflow {
    pub id: String,
    pub title: String,
    pub steps: Vec<WorkflowStep>,
    pub default_condition: String,
}

impl Workflow {
    pub fn generate_log(&self, step: usize) -> Result<TenonWorkflowLog> {
        Ok(TenonWorkflowLog {
            id: self.id.clone(),
            content: format!(
                "{} - {} ({} of {})",
                self.title.clone(),
                self.steps
                    .get(step - 1)
                    .ok_or(anyhow!("invalid step number"))?
                    .title
                    .clone(),
                step,
                self.steps.len()
            ),
            step: Some(step),
        })
    }
}

#[derive(Clone, Deserialize)]
pub enum GotoStep {
    Next,
    Step(usize),
}

#[derive(Clone, Deserialize)]
pub struct WorkflowGotoInstruction {
    pub to: GotoStep,
    pub condition: Option<String>,
    pub output: String,
}

#[derive(Clone, Deserialize)]
pub struct WorkflowStep {
    pub title: String,
    #[serde(default)]
    pub instruction: Instruction,
    pub goto_instructions: Vec<WorkflowGotoInstruction>,
}
