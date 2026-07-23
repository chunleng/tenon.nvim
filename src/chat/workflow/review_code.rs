use super::{
    GotoStep, Instruction, Workflow, WorkflowGotoInstruction, WorkflowStep, workflow_path,
};

pub fn workflow() -> Workflow {
    Workflow {
        id: "review_code".to_string(),
        title: "Review Code".to_string(),
        steps: vec![
            WorkflowStep {
                title: "Setup Diff Source".to_string(),
                instruction: Instruction::File {
                    file: workflow_path("review_code/1_setup_diff_source.md"),
                },
                goto_instructions: vec![WorkflowGotoInstruction {
                    to: GotoStep::Next,
                    condition: None,
                    output_to_workflow_memory: Some("diff_source".to_string()),
                }],
            },
            WorkflowStep {
                title: "Generate Findings".to_string(),
                instruction: Instruction::File {
                    file: workflow_path("review_code/2_generate_findings.md"),
                },
                goto_instructions: vec![WorkflowGotoInstruction {
                    to: GotoStep::EndWorkflow,
                    condition: Some(
                        "No findings were generated".to_string(),
                    ),
                    output_to_workflow_memory: None,
                }],
            },
            WorkflowStep {
                title: "Filter & Report".to_string(),
                instruction: Instruction::File {
                    file: workflow_path("review_code/3_filter_and_report.md"),
                },
                goto_instructions: vec![
                    WorkflowGotoInstruction {
                        to: GotoStep::EndWorkflow,
                        condition: Some(
                            "No blockers remain after filtering or discussion".to_string(),
                        ),
                        output_to_workflow_memory: None,
                    },
                    WorkflowGotoInstruction {
                        to: GotoStep::Step(2),
                        condition: Some(
                            "The user indicates they have made code changes to address the blockers".to_string(),
                        ),
                        output_to_workflow_memory: None,
                    },
                ],
            },
        ],
        description: "Review code changes against a diff, report blockers, and iterate with the author through fixes and pushback until LGTM".to_string(),
    }
}
