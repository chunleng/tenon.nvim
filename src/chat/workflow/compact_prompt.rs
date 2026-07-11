use super::{
    GotoStep, Instruction, Workflow, WorkflowGotoInstruction, WorkflowStep, workflow_path,
};

pub fn workflow() -> Workflow {
    Workflow {
        id: "compact_prompt".to_string(),
        title: "Compact Prompt".to_string(),
        steps: vec![
            WorkflowStep {
                title: "Set Goal".to_string(),
                instruction: Instruction::File {
                    file: workflow_path("compact_prompt/1_set_goal.md"),
                },
                goto_instructions: vec![WorkflowGotoInstruction {
                    to: GotoStep::Next,
                    condition: None,
                    output_to_workflow_memory: Some("goal".to_string()),
                }],
            },
            WorkflowStep {
                title: "Hunt Ambiguity".to_string(),
                instruction: Instruction::File {
                    file: workflow_path("compact_prompt/2_ambiguity_hunt.md"),
                },
                goto_instructions: vec![WorkflowGotoInstruction {
                    to: GotoStep::Next,
                    condition: Some("ambiguity resolved".to_string()),
                    output_to_workflow_memory: None,
                }],
            },
            WorkflowStep {
                title: "Change".to_string(),
                instruction: Instruction::File {
                    file: workflow_path("compact_prompt/3_change.md"),
                },
                goto_instructions: vec![WorkflowGotoInstruction {
                    to: GotoStep::Next,
                    condition: None,
                    output_to_workflow_memory: None,
                }],
            },
            WorkflowStep {
                title: "Goal Check".to_string(),
                instruction: Instruction::File {
                    file: workflow_path("compact_prompt/4_goal_check.md"),
                },
                goto_instructions: vec![
                    WorkflowGotoInstruction {
                        to: GotoStep::Step(3),
                        condition: Some("texts can be simpler".to_string()),
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
        description: "Compacts and simplifies text while preserving meaning".to_string(),
    }
}
