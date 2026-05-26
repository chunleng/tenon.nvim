use super::{
    GotoStep, Instruction, Workflow, WorkflowGotoInstruction, WorkflowStep, workflow_path,
};

pub fn workflow() -> Workflow {
    Workflow {
        id: "plan_refactoring".to_string(),
        title: "Plan Refactoring".to_string(),
        steps: vec![
            WorkflowStep {
                title: "Set Goal".to_string(),
                instruction: Instruction::File {
                    file: workflow_path("plan_refactoring/1_set_goal.md"),
                },
                goto_instructions: vec![WorkflowGotoInstruction {
                    to: GotoStep::Next,
                    condition: None,
                    output: "refactoring requirement and constraints".to_string(),
                    output_to_workflow_memory: Some("constraints".to_string()),
                }],
            },
            WorkflowStep {
                title: "Plan".to_string(),
                instruction: Instruction::File {
                    file: workflow_path("plan_refactoring/2_plan.md"),
                },
                goto_instructions: vec![WorkflowGotoInstruction {
                    to: GotoStep::Next,
                    condition: None,
                    output: "refactoring plan with changes".to_string(),
                    output_to_workflow_memory: None,
                }],
            },
            WorkflowStep {
                title: "Prune".to_string(),
                instruction: Instruction::File {
                    file: workflow_path("plan_refactoring/3_prune.md"),
                },
                goto_instructions: vec![WorkflowGotoInstruction {
                    to: GotoStep::Next,
                    condition: None,
                    output: "pruned refactoring plan".to_string(),
                    output_to_workflow_memory: None,
                }],
            },
            WorkflowStep {
                title: "Verify".to_string(),
                instruction: Instruction::File {
                    file: workflow_path("plan_refactoring/4_verify.md"),
                },
                goto_instructions: vec![
                    WorkflowGotoInstruction {
                        to: GotoStep::Step(2),
                        condition: Some("verification failed".to_string()),
                        output: "issues found in plan".to_string(),
                        output_to_workflow_memory: None,
                    },
                    WorkflowGotoInstruction {
                        to: GotoStep::EndWorkflow,
                        condition: None,
                        output: "verified refactoring plan ready for implement_software"
                            .to_string(),
                        output_to_workflow_memory: None,
                    },
                ],
            },
        ],
        default_condition: "when user wants to refactor code".to_string(),
    }
}
