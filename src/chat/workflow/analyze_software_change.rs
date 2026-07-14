use super::{
    GotoStep, Instruction, Workflow, WorkflowGotoInstruction, WorkflowStep, workflow_path,
};

pub fn workflow() -> Workflow {
    Workflow {
        id: "analyze_software_change".to_string(),
        title: "Analyze Software Change".to_string(),
        steps: vec![
            WorkflowStep {
                title: "Scope".to_string(),
                instruction: Instruction::File {
                    file: workflow_path("analyze_software_change/1_scope.md"),
                },
                goto_instructions: vec![WorkflowGotoInstruction {
                    to: GotoStep::Next,
                    condition: None,
                    output_to_workflow_memory: Some("scope".to_string()),
                }],
            },
            WorkflowStep {
                title: "Plan".to_string(),
                instruction: Instruction::File {
                    file: workflow_path("analyze_software_change/2_plan.md"),
                },
                goto_instructions: vec![WorkflowGotoInstruction {
                    to: GotoStep::Next,
                    condition: None,
                    output_to_workflow_memory: Some("plan".to_string()),
                }],
            },
            WorkflowStep {
                title: "Validate".to_string(),
                instruction: Instruction::File {
                    file: workflow_path("analyze_software_change/3_validate.md"),
                },
                goto_instructions: vec![
                    WorkflowGotoInstruction {
                        to: GotoStep::Step(1),
                        condition: Some("scope issues are found".to_string()),
                        output_to_workflow_memory: None,
                    },
                    WorkflowGotoInstruction {
                        to: GotoStep::Step(2),
                        condition: Some("plan issues are found".to_string()),
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
        description: "Scope a change request through codebase investigation and user interview, then plan an ordered sequence of non-breaking, user-visible milestone steps, and validate coverage against scope".to_string(),
    }
}
