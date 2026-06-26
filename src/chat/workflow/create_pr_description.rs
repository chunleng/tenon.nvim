use super::{
    GotoStep, Instruction, Workflow, WorkflowGotoInstruction, WorkflowStep, workflow_path,
};

pub fn workflow() -> Workflow {
    Workflow {
        id: "create_pr_description".to_string(),
        title: "Create PR Description".to_string(),
        steps: vec![
            WorkflowStep {
                title: "Gather Diff".to_string(),
                instruction: Instruction::File {
                    file: workflow_path("create_pr_description/1_gather_diff.md"),
                },
                goto_instructions: vec![WorkflowGotoInstruction {
                    to: GotoStep::Next,
                    condition: None,
                    output_to_workflow_memory: Some("diff".to_string()),
                }],
            },
            WorkflowStep {
                title: "Analyze Changes".to_string(),
                instruction: Instruction::File {
                    file: workflow_path("create_pr_description/2_analyze_changes.md"),
                },
                goto_instructions: vec![WorkflowGotoInstruction {
                    to: GotoStep::Next,
                    condition: None,
                    output_to_workflow_memory: Some("analysis".to_string()),
                }],
            },
            WorkflowStep {
                title: "Generate Title & Description".to_string(),
                instruction: Instruction::File {
                    file: workflow_path("create_pr_description/3_generate.md"),
                },
                goto_instructions: vec![WorkflowGotoInstruction {
                    to: GotoStep::EndWorkflow,
                    condition: None,
                    output_to_workflow_memory: None,
                }],
            },
        ],
        default_condition: "when user wants to generate or create a PR title and description"
            .to_string(),
    }
}
