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
                    output_to_workflow_memory: Some("plan".to_string()),
                }],
            },
            WorkflowStep {
                title: "Implement".to_string(),
                instruction: Instruction::File {
                    file: workflow_path("implement_code/3_implement.md"),
                },
                goto_instructions: vec![WorkflowGotoInstruction {
                    to: GotoStep::Next,
                    condition: Some(
                        "confirmed valid code and fixed error from linter/compiler".to_string()
                    ),
                    output_to_workflow_memory: None,
                }],
            },
            WorkflowStep {
                title: "Verify".to_string(),
                instruction: Instruction::File {
                    file: workflow_path("implement_code/4_verify.md"),
                },
                goto_instructions: vec![
                    WorkflowGotoInstruction {
                        to: GotoStep::Step(3),
                        condition: Some("verification fails".to_string()),
                        output_to_workflow_memory: None,
                    },
                    WorkflowGotoInstruction {
                        to: GotoStep::Next,
                        condition: None,
                        output_to_workflow_memory: Some("unverifiable".to_string()),
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
                title: "Finalize".to_string(),
                instruction: Instruction::File {
                    file: workflow_path("implement_code/6_finalize.md"),
                },
                goto_instructions: vec![
                    WorkflowGotoInstruction {
                        to: GotoStep::Step(2),
                        condition: Some("tests fail".to_string()),
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
        description: "Implements code changes through upfront planning, implement-then-verify cycles, and deviation-aware re-planning".to_string(),
    }
}
