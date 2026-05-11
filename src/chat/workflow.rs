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

pub fn load_system_workflows() -> Vec<Workflow> {
    vec![Workflow {
        id: "find_software_bug_root_cause".to_string(),
        title: "Find Software Bug Root Cause".to_string(),
        steps: vec![
            WorkflowStep {
                title: "Define".to_string(),
                instruction: Instruction::File {
                    file: workflow_path("find_software_bug_root_cause/1_define.md"),
                },
                goto_instructions: vec![WorkflowGotoInstruction {
                    to: GotoStep::Next,
                    condition: None,
                    output: "list of bug definition".to_string(),
                }],
            },
            WorkflowStep {
                title: "Locate".to_string(),
                instruction: Instruction::File {
                    file: workflow_path("find_software_bug_root_cause/2_locate.md"),
                },
                goto_instructions: vec![
                    WorkflowGotoInstruction {
                        to: GotoStep::Step(1),
                        condition: Some("unable to locate".to_string()),
                        output: "reason why unable to locate".to_string(),
                    },
                    WorkflowGotoInstruction {
                        to: GotoStep::Next,
                        condition: None,
                        output: "list of files+explanation related to bug".to_string(),
                    },
                ],
            },
            WorkflowStep {
                title: "Reproduce".to_string(),
                instruction: Instruction::File {
                    file: workflow_path("find_software_bug_root_cause/3_reproduce.md"),
                },
                goto_instructions: vec![
                    WorkflowGotoInstruction {
                        to: GotoStep::Step(1),
                        condition: Some("unable to create test".to_string()),
                        output: "reason why unable to create test".to_string(),
                    },
                    WorkflowGotoInstruction {
                        to: GotoStep::Next,
                        condition: None,
                        output: "list of test case".to_string(),
                    },
                ],
            },
            WorkflowStep {
                title: "Cleanup".to_string(),
                instruction: Instruction::File {
                    file: workflow_path("find_software_bug_root_cause/4_cleanup.md"),
                },
                goto_instructions: vec![WorkflowGotoInstruction {
                    to: GotoStep::Next,
                    condition: None,
                    output: "cleanup done".to_string(),
                }],
            },
            WorkflowStep {
                title: "Conclude".to_string(),
                instruction: Instruction::File {
                    file: workflow_path("find_software_bug_root_cause/5_conclude.md"),
                },
                goto_instructions: vec![WorkflowGotoInstruction {
                    to: GotoStep::EndWorkflow,
                    condition: None,
                    output: "analysis of the bug".to_string(),
                }],
            },
        ],
        default_condition: "before trying to resolve a bug".to_string(),
    }]
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
    EndWorkflow,
}

impl GotoStep {
    /// Resolves this goto target to a concrete step index.
    /// Returns `Some(step_index)` for `Next` (based on current_step) and `Step(n)`.
    /// Returns `None` for `EndWorkflow` (not a step-based target).
    pub fn resolve_step_index(&self, current_step: usize) -> Option<usize> {
        match self {
            GotoStep::Next => Some(current_step + 1),
            GotoStep::Step(n) => Some(*n),
            GotoStep::EndWorkflow => None,
        }
    }
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
