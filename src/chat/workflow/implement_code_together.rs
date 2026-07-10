use super::{
    GotoStep, Instruction, Workflow, WorkflowGotoInstruction, WorkflowStep, workflow_path,
};

pub fn workflow() -> Workflow {
    Workflow {
        id: "implement_code_together".to_string(),
        title: "Implement Code Together".to_string(),
        steps: vec![
            WorkflowStep {
                title: "Listen for Next Goal".to_string(),
                instruction: Instruction::File {
                    file: workflow_path("implement_code_together/1_listen.md"),
                },
                goto_instructions: vec![
                    WorkflowGotoInstruction {
                        to: GotoStep::EndWorkflow,
                        condition: Some("user says stop".to_string()),
                        output_to_workflow_memory: None,
                    },
                    WorkflowGotoInstruction {
                        to: GotoStep::Next,
                        condition: Some("confirmed with user".to_string()),
                        output_to_workflow_memory: Some("goal".to_string()),
                    },
                ],
            },
            WorkflowStep {
                title: "Prepare Test".to_string(),
                instruction: Instruction::File {
                    file: workflow_path("implement_code_together/2_prepare_test.md"),
                },
                goto_instructions: vec![WorkflowGotoInstruction {
                    to: GotoStep::Next,
                    condition: Some("confirmed with user".to_string()),
                    output_to_workflow_memory: None,
                }],
            },
            WorkflowStep {
                title: "Implement".to_string(),
                instruction: Instruction::File {
                    file: workflow_path("implement_code_together/3_implement.md"),
                },
                goto_instructions: vec![WorkflowGotoInstruction {
                    to: GotoStep::Step(1),
                    condition: None,
                    output_to_workflow_memory: None,
                }],
            },
        ],
        description: "Collaboratively implements code changes with the user turn-by-turn"
            .to_string(),
    }
}
