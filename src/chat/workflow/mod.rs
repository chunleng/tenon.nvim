mod compact_prompt;
mod create_pr_description;
mod create_workflow;
mod edit_document;
mod find_software_bug_root_cause;
mod implement_code;
mod implement_code_together;
mod plan_refactoring;
mod plan_software_change;

use anyhow::{Result, anyhow};
use serde::Deserialize;
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};

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
            Instruction::File { file } => std::fs::read_to_string(file).map_err(|e| {
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

pub fn load_system_workflows() -> Vec<Arc<Workflow>> {
    vec![
        Arc::new(find_software_bug_root_cause::workflow()),
        Arc::new(create_pr_description::workflow()),
        Arc::new(create_workflow::workflow()),
        Arc::new(edit_document::workflow()),
        Arc::new(implement_code::workflow()),
        Arc::new(plan_refactoring::workflow()),
        Arc::new(plan_software_change::workflow()),
        Arc::new(compact_prompt::workflow()),
        Arc::new(implement_code_together::workflow()),
    ]
}

#[derive(Debug, Clone, Deserialize)]
pub struct Workflow {
    pub id: String,
    pub title: String,
    pub steps: Vec<WorkflowStep>,
    pub description: String,
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

#[derive(Debug, Clone, Deserialize)]
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

#[derive(Debug, Clone, Deserialize)]
pub struct WorkflowGotoInstruction {
    pub to: GotoStep,
    pub condition: Option<String>,
    #[serde(default)]
    pub output_to_workflow_memory: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct WorkflowStep {
    pub title: String,
    #[serde(default)]
    pub instruction: Instruction,
    pub goto_instructions: Vec<WorkflowGotoInstruction>,
}
