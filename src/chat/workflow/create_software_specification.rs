use super::{
    GotoStep, Instruction, Workflow, WorkflowGotoInstruction, WorkflowStep, workflow_path,
};

pub fn workflow() -> Workflow {
    Workflow {
        id: "create_software_specification".to_string(),
        title: "Create Software Specification".to_string(),
        steps: vec![
            WorkflowStep {
                title: "Frame".to_string(),
                instruction: Instruction::File {
                    file: workflow_path("create_software_specification/1_frame.md"),
                },
                goto_instructions: vec![WorkflowGotoInstruction {
                    to: GotoStep::Next,
                    condition: None,
                    output_to_workflow_memory: Some("decision_frame".to_string()),
                }],
            },
            WorkflowStep {
                title: "Explore".to_string(),
                instruction: Instruction::File {
                    file: workflow_path("create_software_specification/2_explore.md"),
                },
                goto_instructions: vec![],
            },
            WorkflowStep {
                title: "Draft".to_string(),
                instruction: Instruction::File {
                    file: workflow_path("create_software_specification/3_draft.md"),
                },
                goto_instructions: vec![],
            },
            WorkflowStep {
                title: "Review".to_string(),
                instruction: Instruction::File {
                    file: workflow_path("create_software_specification/4_review.md"),
                },
                goto_instructions: vec![WorkflowGotoInstruction {
                    to: GotoStep::Step(3),
                    condition: Some(
                        "document not decision-ready and iteration under 3".to_string(),
                    ),
                    output_to_workflow_memory: Some("gaps".to_string()),
                }],
            },
        ],
        description: "Creates or refines decision-alignment documents (specs, PRDs, RFCs, ADRs, \
            design docs) to help a reader evaluate or align on a decision"
            .to_string(),
    }
}
