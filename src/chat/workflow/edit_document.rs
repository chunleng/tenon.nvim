use super::{
    GotoStep, Instruction, Workflow, WorkflowGotoInstruction, WorkflowStep, workflow_path,
};

pub fn workflow() -> Workflow {
    Workflow {
        id: "edit_document".to_string(),
        title: "Edit Document".to_string(),
        steps: vec![
            WorkflowStep {
                title: "Gather".to_string(),
                instruction: Instruction::File {
                    file: workflow_path("edit_document/1_gather.md"),
                },
                goto_instructions: vec![WorkflowGotoInstruction {
                    to: GotoStep::Next,
                    condition: None,
                    output_to_workflow_memory: None,
                }],
            },
            WorkflowStep {
                title: "Set Goal".to_string(),
                instruction: Instruction::File {
                    file: workflow_path("edit_document/2_set_goal.md"),
                },
                goto_instructions: vec![WorkflowGotoInstruction {
                    to: GotoStep::Next,
                    condition: None,
                    output_to_workflow_memory: Some("goal".to_string()),
                }],
            },
            WorkflowStep {
                title: "Execute".to_string(),
                instruction: Instruction::File {
                    file: workflow_path("edit_document/3_execute.md"),
                },
                goto_instructions: vec![WorkflowGotoInstruction {
                    to: GotoStep::Next,
                    condition: None,
                    output_to_workflow_memory: None,
                }],
            },
            WorkflowStep {
                title: "Refine".to_string(),
                instruction: Instruction::File {
                    file: workflow_path("edit_document/4_refine.md"),
                },
                goto_instructions: vec![WorkflowGotoInstruction {
                    to: GotoStep::Next,
                    condition: None,
                    output_to_workflow_memory: None,
                }],
            },
            WorkflowStep {
                title: "Check Goal".to_string(),
                instruction: Instruction::File {
                    file: workflow_path("edit_document/5_check_goal.md"),
                },
                goto_instructions: vec![
                    WorkflowGotoInstruction {
                        to: GotoStep::Step(3),
                        condition: Some("goal not achieved and iteration under 3".to_string()),
                        output_to_workflow_memory: Some("gaps".to_string()),
                    },
                    WorkflowGotoInstruction {
                        to: GotoStep::EndWorkflow,
                        condition: None,
                        output_to_workflow_memory: None,
                    },
                ],
            },
        ],
        description:
            "Creates or updates documentation (e.g., README.md, doc folders, markdown files)"
                .to_string(),
    }
}
