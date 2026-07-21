use super::{
    GotoStep, Instruction, Workflow, WorkflowGotoInstruction, WorkflowStep, workflow_path,
};

pub fn workflow() -> Workflow {
    Workflow {
        id: "edit_workflow".to_string(),
        title: "Edit Workflow".to_string(),
        steps: vec![
            WorkflowStep {
                title: "Setup".to_string(),
                instruction: Instruction::File {
                    file: workflow_path("edit_workflow/1_setup.md"),
                },
                goto_instructions: vec![WorkflowGotoInstruction {
                    to: GotoStep::Next,
                    condition: None,
                    output_to_workflow_memory: Some("setup".to_string()),
                }],
            },
            WorkflowStep {
                title: "Gather Requirements".to_string(),
                instruction: Instruction::File {
                    file: workflow_path("edit_workflow/2_gather_requirements.md"),
                },
                goto_instructions: vec![WorkflowGotoInstruction {
                    to: GotoStep::Next,
                    condition: None,
                    output_to_workflow_memory: Some("requirements".to_string()),
                }],
            },
            WorkflowStep {
                title: "Design Steps".to_string(),
                instruction: Instruction::File {
                    file: workflow_path("edit_workflow/3_design_steps.md"),
                },
                goto_instructions: vec![WorkflowGotoInstruction {
                    to: GotoStep::Next,
                    condition: None,
                    output_to_workflow_memory: Some("step_design".to_string()),
                }],
            },
            WorkflowStep {
                title: "Draft".to_string(),
                instruction: Instruction::File {
                    file: workflow_path("edit_workflow/4_draft.md"),
                },
                goto_instructions: vec![
                    WorkflowGotoInstruction {
                        to: GotoStep::Step(3),
                        condition: Some("feedback involves step design changes".to_string()),
                        output_to_workflow_memory: None,
                    },
                ],
            },
            WorkflowStep {
                title: "Review".to_string(),
                instruction: Instruction::File {
                    file: workflow_path("edit_workflow/5_review.md"),
                },
                goto_instructions: vec![
                    WorkflowGotoInstruction {
                        to: GotoStep::Step(3),
                        condition: Some("structural issues found".to_string()),
                        output_to_workflow_memory: None,
                    },
                    WorkflowGotoInstruction {
                        to: GotoStep::Step(4),
                        condition: Some("content issues found".to_string()),
                        output_to_workflow_memory: None,
                    },
                ],
            },
        ],
        description: "Create or update Tenon workflows through collaborative goal-setting, step isolation criteria, and impact-aware editing".to_string(),
    }
}
