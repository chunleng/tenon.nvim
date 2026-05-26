use super::{
    GotoStep, Instruction, Workflow, WorkflowGotoInstruction, WorkflowStep, workflow_path,
};

pub fn workflow() -> Workflow {
    Workflow {
        id: "implement_code".to_string(),
        title: "Implement Code".to_string(),
        steps: vec![
            WorkflowStep {
                title: "Understand".to_string(),
                instruction: Instruction::File {
                    file: workflow_path("implement_code/1_understand.md"),
                },
                goto_instructions: vec![WorkflowGotoInstruction {
                    to: GotoStep::Next,
                    condition: None,
                    output: "requirements with acceptance criteria (each with verification method)"
                        .to_string(),
                    output_to_workflow_memory: Some("goal".to_string()),
                }],
            },
            WorkflowStep {
                title: "Plan".to_string(),
                instruction: Instruction::File {
                    file: workflow_path("implement_code/2_plan.md"),
                },
                goto_instructions: vec![WorkflowGotoInstruction {
                    to: GotoStep::Next,
                    condition: None,
                    output: "next incremental change to make".to_string(),
                    output_to_workflow_memory: None,
                }],
            },
            WorkflowStep {
                title: "Prepare Test".to_string(),
                instruction: Instruction::File {
                    file: workflow_path("implement_code/3_prepare_test.md"),
                },
                goto_instructions: vec![WorkflowGotoInstruction {
                    to: GotoStep::Next,
                    condition: None,
                    output: "test that fails before change, passes after".to_string(),
                    output_to_workflow_memory: None,
                }],
            },
            WorkflowStep {
                title: "Implement".to_string(),
                instruction: Instruction::File {
                    file: workflow_path("implement_code/4_implement.md"),
                },
                goto_instructions: vec![
                    WorkflowGotoInstruction {
                        to: GotoStep::Step(4),
                        condition: Some("verification failed".to_string()),
                        output: "failure details".to_string(),
                        output_to_workflow_memory: None,
                    },
                    WorkflowGotoInstruction {
                        to: GotoStep::Next,
                        condition: None,
                        output: "build result + affected test results".to_string(),
                        output_to_workflow_memory: None,
                    },
                ],
            },
            WorkflowStep {
                title: "Goal Check".to_string(),
                instruction: Instruction::File {
                    file: workflow_path("implement_code/5_goal_check.md"),
                },
                goto_instructions: vec![
                    WorkflowGotoInstruction {
                        to: GotoStep::Step(2),
                        condition: Some("goal not reached".to_string()),
                        output: "remaining gap".to_string(),
                        output_to_workflow_memory: None,
                    },
                    WorkflowGotoInstruction {
                        to: GotoStep::Next,
                        condition: None,
                        output: "goal reached: true/false with reasoning".to_string(),
                        output_to_workflow_memory: None,
                    },
                ],
            },
            WorkflowStep {
                title: "Cleanup".to_string(),
                instruction: Instruction::File {
                    file: workflow_path("implement_code/6_cleanup.md"),
                },
                goto_instructions: vec![WorkflowGotoInstruction {
                    to: GotoStep::Next,
                    condition: None,
                    output: "cleaned codebase".to_string(),
                    output_to_workflow_memory: None,
                }],
            },
            WorkflowStep {
                title: "Finalize".to_string(),
                instruction: Instruction::File {
                    file: workflow_path("implement_code/7_finalize.md"),
                },
                goto_instructions: vec![
                    WorkflowGotoInstruction {
                        to: GotoStep::Step(2),
                        condition: Some("tests fail".to_string()),
                        output: "failed tests list".to_string(),
                        output_to_workflow_memory: None,
                    },
                    WorkflowGotoInstruction {
                        to: GotoStep::EndWorkflow,
                        condition: None,
                        output: "all tests pass + unverifiable aspects documented".to_string(),
                        output_to_workflow_memory: None,
                    },
                ],
            },
        ],
        default_condition: "when user requests code impl/changes".to_string(),
    }
}
