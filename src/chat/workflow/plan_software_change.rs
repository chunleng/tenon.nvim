use super::{
    GotoStep, Instruction, Workflow, WorkflowGotoInstruction, WorkflowStep, workflow_path,
};

pub fn workflow() -> Workflow {
    Workflow {
        id: "plan_software_change".to_string(),
        title: "Plan Software Change".to_string(),
        steps: vec![
            WorkflowStep {
                title: "Elicit Requirements".to_string(),
                instruction: Instruction::File {
                    file: workflow_path("plan_software_change/1_elicit.md"),
                },
                goto_instructions: vec![WorkflowGotoInstruction {
                    to: GotoStep::Next,
                    condition: None,
                    output_to_workflow_memory: Some("requirement".to_string()),
                }],
            },
            WorkflowStep {
                title: "Verify Requirements".to_string(),
                instruction: Instruction::File {
                    file: workflow_path("plan_software_change/2_verify.md"),
                },
                goto_instructions: vec![
                    WorkflowGotoInstruction {
                        to: GotoStep::Step(1),
                        condition: Some("verification failed".to_string()),
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
                title: "Analyze Changes".to_string(),
                instruction: Instruction::File {
                    file: workflow_path("plan_software_change/3_analyze.md"),
                },
                goto_instructions: vec![WorkflowGotoInstruction {
                    to: GotoStep::Next,
                    condition: None,
                    output_to_workflow_memory: None,
                }],
            },
            WorkflowStep {
                title: "Prune".to_string(),
                instruction: Instruction::File {
                    file: workflow_path("plan_software_change/4_prune.md"),
                },
                goto_instructions: vec![WorkflowGotoInstruction {
                    to: GotoStep::Next,
                    condition: None,
                    output_to_workflow_memory: None,
                }],
            },
            WorkflowStep {
                title: "Verify Plan".to_string(),
                instruction: Instruction::File {
                    file: workflow_path("plan_software_change/5_verify_plan.md"),
                },
                goto_instructions: vec![
                    WorkflowGotoInstruction {
                        to: GotoStep::Step(3),
                        condition: Some("verification failed".to_string()),
                        output_to_workflow_memory: None,
                    },
                    WorkflowGotoInstruction {
                        to: GotoStep::EndWorkflow,
                        condition: None,
                        output_to_workflow_memory: None,
                    },
                ],
            },
        ],
        description: "Plans software changes for new features, behavior modifications, or greenfield projects".to_string(),
    }
}
