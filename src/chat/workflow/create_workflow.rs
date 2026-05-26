use super::{
    GotoStep, Instruction, Workflow, WorkflowGotoInstruction, WorkflowStep, workflow_path,
};

pub fn workflow() -> Workflow {
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
                    output_to_workflow_memory: Some("goal".to_string()),
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
                    output: "nothing".to_string(),
                    output_to_workflow_memory: None,
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
                    output_to_workflow_memory: None,
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
                    output_to_workflow_memory: None,
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
                        output_to_workflow_memory: None,
                    },
                    WorkflowGotoInstruction {
                        to: GotoStep::Next,
                        condition: None,
                        output: "validated workflow".to_string(),
                        output_to_workflow_memory: None,
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
                        output_to_workflow_memory: None,
                    },
                    WorkflowGotoInstruction {
                        to: GotoStep::EndWorkflow,
                        condition: None,
                        output: "final workflow".to_string(),
                        output_to_workflow_memory: None,
                    },
                ],
            },
        ],
        default_condition: "when user wants to create a workflow (agent-prompting related only)"
            .to_string(),
    }
}
