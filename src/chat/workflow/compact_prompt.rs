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
                title: "Classify & Resolve".to_string(),
                instruction: Instruction::File {
                    file: workflow_path("compact_prompt/2_classify.md"),
                },
                goto_instructions: vec![WorkflowGotoInstruction {
                    to: GotoStep::Next,
                    condition: None,
                    output_to_workflow_memory: Some("classification".to_string()),
                }],
            },
            WorkflowStep {
                title: "Compact".to_string(),
                instruction: Instruction::File {
                    file: workflow_path("compact_prompt/3_compact.md"),
                },
                goto_instructions: vec![],
            },
            WorkflowStep {
                title: "Verify".to_string(),
                instruction: Instruction::File {
                    file: workflow_path("compact_prompt/4_verify.md"),
                },
                goto_instructions: vec![WorkflowGotoInstruction {
                    to: GotoStep::Step(3),
                    condition: Some("compaction issues found".to_string()),
                    output_to_workflow_memory: None,
                }],
            },
        ],
        description: "Compacts prompt and directive text while preserving meaning, conditions, and constraints".to_string(),
    }
}
