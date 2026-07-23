use super::{
    GotoStep, Instruction, Workflow, WorkflowGotoInstruction, WorkflowStep, workflow_path,
};

pub fn workflow() -> Workflow {
    Workflow {
        id: "edit_directive".to_string(),
        title: "Edit Directive".to_string(),
        steps: vec![
            WorkflowStep {
                title: "Investigate".to_string(),
                instruction: Instruction::File {
                    file: workflow_path("edit_directive/1_investigate.md"),
                },
                goto_instructions: vec![WorkflowGotoInstruction {
                    to: GotoStep::Next,
                    condition: None,
                    output_to_workflow_memory: Some("root_cause".to_string()),
                }],
            },
            WorkflowStep {
                title: "Analyze".to_string(),
                instruction: Instruction::File {
                    file: workflow_path("edit_directive/2_analyze.md"),
                },
                goto_instructions: vec![
                    WorkflowGotoInstruction {
                        to: GotoStep::EndWorkflow,
                        condition: Some("no directive needed".to_string()),
                        output_to_workflow_memory: None,
                    },
                    WorkflowGotoInstruction {
                        to: GotoStep::Next,
                        condition: None,
                        output_to_workflow_memory: Some("decision".to_string()),
                    },
                ],
            },
            WorkflowStep {
                title: "Draft".to_string(),
                instruction: Instruction::File {
                    file: workflow_path("edit_directive/3_draft.md"),
                },
                goto_instructions: vec![],
            },
        ],
        description: "Create or update directives by investigating agent behavior problems, identifying root causes, and drafting targeted behavior-steering or knowledge-boosting rules".to_string(),
    }
}
