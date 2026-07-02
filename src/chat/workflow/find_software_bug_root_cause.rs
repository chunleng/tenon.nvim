use super::{
    GotoStep, Instruction, Workflow, WorkflowGotoInstruction, WorkflowStep, workflow_path,
};

pub fn workflow() -> Workflow {
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
                    output_to_workflow_memory: None,
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
                        output_to_workflow_memory: None,
                    },
                    WorkflowGotoInstruction {
                        to: GotoStep::Next,
                        condition: None,
                        output_to_workflow_memory: None,
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
                        output_to_workflow_memory: None,
                    },
                    WorkflowGotoInstruction {
                        to: GotoStep::Next,
                        condition: None,
                        output_to_workflow_memory: None,
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
                    output_to_workflow_memory: None,
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
                    output_to_workflow_memory: None,
                }],
            },
        ],
        description: "Finds the root cause of a software bug through systematic investigation"
            .to_string(),
    }
}
