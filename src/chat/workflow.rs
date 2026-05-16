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

pub fn load_system_workflows() -> Vec<Workflow> {
    vec![
        Workflow {
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
        },
        Workflow {
            id: "create_workflow".to_string(),
            title: "Create Workflow".to_string(),
            steps: vec![
                WorkflowStep {
                    title: "Define Goal & Steps".to_string(),
                    instruction: Instruction::File {
                        file: workflow_path("create_workflow/1_define.md"),
                    },
                    goto_instructions: vec![WorkflowGotoInstruction {
                        to: GotoStep::Next,
                        condition: None,
                        output: "workflow goal and step definitions".to_string(),
                    }],
                },
                WorkflowStep {
                    title: "Draft Workflow".to_string(),
                    instruction: Instruction::File {
                        file: workflow_path("create_workflow/2_draft.md"),
                    },
                    goto_instructions: vec![WorkflowGotoInstruction {
                        to: GotoStep::Next,
                        condition: None,
                        output: "drafted workflow".to_string(),
                    }],
                },
                WorkflowStep {
                    title: "Flag Vague Lines".to_string(),
                    instruction: Instruction::File {
                        file: workflow_path("create_workflow/3_flag.md"),
                    },
                    goto_instructions: vec![WorkflowGotoInstruction {
                        to: GotoStep::Next,
                        condition: None,
                        output: "flagged lines with reasons (or none if no issues)".to_string(),
                    }],
                },
                WorkflowStep {
                    title: "Prune Flagged Lines".to_string(),
                    instruction: Instruction::File {
                        file: workflow_path("create_workflow/4_prune.md"),
                    },
                    goto_instructions: vec![WorkflowGotoInstruction {
                        to: GotoStep::Next,
                        condition: None,
                        output: "pruned workflow".to_string(),
                    }],
                },
                WorkflowStep {
                    title: "Validate Flows".to_string(),
                    instruction: Instruction::File {
                        file: workflow_path("create_workflow/5_validate.md"),
                    },
                    goto_instructions: vec![
                        WorkflowGotoInstruction {
                            to: GotoStep::Step(2),
                            condition: Some("flow issues found".to_string()),
                            output: "flow issues description".to_string(),
                        },
                        WorkflowGotoInstruction {
                            to: GotoStep::Next,
                            condition: None,
                            output: "validated workflow".to_string(),
                        },
                    ],
                },
                WorkflowStep {
                    title: "Review & Finalize".to_string(),
                    instruction: Instruction::File {
                        file: workflow_path("create_workflow/6_finalize.md"),
                    },
                    goto_instructions: vec![
                        WorkflowGotoInstruction {
                            to: GotoStep::Step(2),
                            condition: Some("user requests changes".to_string()),
                            output: "change requests".to_string(),
                        },
                        WorkflowGotoInstruction {
                            to: GotoStep::EndWorkflow,
                            condition: None,
                            output: "final workflow".to_string(),
                        },
                    ],
                },
            ],
            default_condition:
                "when user wants to create a workflow (agent-prompting related only)".to_string(),
        },
        Workflow {
            id: "implement_software".to_string(),
            title: "Implement Software".to_string(),
            steps: vec![
                WorkflowStep {
                    title: "Understand".to_string(),
                    instruction: Instruction::File {
                        file: workflow_path("implement_software/1_understand.md"),
                    },
                    goto_instructions: vec![WorkflowGotoInstruction {
                        to: GotoStep::Next,
                        condition: None,
                        output:
                            "requirements with acceptance criteria (each with verification method)"
                                .to_string(),
                    }],
                },
                WorkflowStep {
                    title: "Plan".to_string(),
                    instruction: Instruction::File {
                        file: workflow_path("implement_software/2_plan.md"),
                    },
                    goto_instructions: vec![WorkflowGotoInstruction {
                        to: GotoStep::Next,
                        condition: None,
                        output: "next incremental change to make".to_string(),
                    }],
                },
                WorkflowStep {
                    title: "Prepare Test".to_string(),
                    instruction: Instruction::File {
                        file: workflow_path("implement_software/3_prepare_test.md"),
                    },
                    goto_instructions: vec![WorkflowGotoInstruction {
                        to: GotoStep::Next,
                        condition: None,
                        output: "test that fails before change, passes after".to_string(),
                    }],
                },
                WorkflowStep {
                    title: "Implement".to_string(),
                    instruction: Instruction::File {
                        file: workflow_path("implement_software/4_implement.md"),
                    },
                    goto_instructions: vec![
                        WorkflowGotoInstruction {
                            to: GotoStep::Step(4),
                            condition: Some("verification failed".to_string()),
                            output: "failure details".to_string(),
                        },
                        WorkflowGotoInstruction {
                            to: GotoStep::Next,
                            condition: None,
                            output: "build result + affected test results".to_string(),
                        },
                    ],
                },
                WorkflowStep {
                    title: "Goal Check".to_string(),
                    instruction: Instruction::File {
                        file: workflow_path("implement_software/5_goal_check.md"),
                    },
                    goto_instructions: vec![
                        WorkflowGotoInstruction {
                            to: GotoStep::Step(2),
                            condition: Some("goal not reached".to_string()),
                            output: "remaining gap".to_string(),
                        },
                        WorkflowGotoInstruction {
                            to: GotoStep::Next,
                            condition: None,
                            output: "goal reached: true/false with reasoning".to_string(),
                        },
                    ],
                },
                WorkflowStep {
                    title: "Cleanup".to_string(),
                    instruction: Instruction::File {
                        file: workflow_path("implement_software/6_cleanup.md"),
                    },
                    goto_instructions: vec![WorkflowGotoInstruction {
                        to: GotoStep::Next,
                        condition: None,
                        output: "cleaned codebase".to_string(),
                    }],
                },
                WorkflowStep {
                    title: "Finalize".to_string(),
                    instruction: Instruction::File {
                        file: workflow_path("implement_software/7_finalize.md"),
                    },
                    goto_instructions: vec![
                        WorkflowGotoInstruction {
                            to: GotoStep::Step(2),
                            condition: Some("tests fail".to_string()),
                            output: "failed tests list".to_string(),
                        },
                        WorkflowGotoInstruction {
                            to: GotoStep::EndWorkflow,
                            condition: None,
                            output: "all tests pass + unverifiable aspects documented".to_string(),
                        },
                    ],
                },
            ],
            default_condition: "before modifying files that are production code".to_string(),
        },
    ]
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
