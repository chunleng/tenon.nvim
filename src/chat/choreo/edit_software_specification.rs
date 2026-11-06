use super::{Choreo, GotoMove, Instruction, Move, MoveGotoInstruction, choreo_path};

pub fn choreo() -> Choreo {
    Choreo {
        id: "edit_software_specification".to_string(),
        title: "Edit Software Specification".to_string(),
        moves: vec![
            Move {
                title: "Frame".to_string(),
                instruction: Instruction::File {
                    file: choreo_path("edit_software_specification/1_frame.md"),
                },
                goto_instructions: vec![MoveGotoInstruction {
                    to: GotoMove::Next,
                    condition: None,
                    output_to_choreo_memory: Some("decision_frame".to_string()),
                }],
            },
            Move {
                title: "Explore".to_string(),
                instruction: Instruction::File {
                    file: choreo_path("edit_software_specification/2_explore.md"),
                },
                goto_instructions: vec![MoveGotoInstruction {
                    to: GotoMove::Next,
                    condition: None,
                    output_to_choreo_memory: Some("exploration".to_string()),
                }],
            },
            Move {
                title: "Draft".to_string(),
                instruction: Instruction::File {
                    file: choreo_path("edit_software_specification/3_draft.md"),
                },
                goto_instructions: vec![MoveGotoInstruction {
                    to: GotoMove::Move(4),
                    condition: Some("user confirms the draft".to_string()),
                    output_to_choreo_memory: None,
                }],
            },
            Move {
                title: "Review".to_string(),
                instruction: Instruction::File {
                    file: choreo_path("edit_software_specification/4_review.md"),
                },
                goto_instructions: vec![MoveGotoInstruction {
                    to: GotoMove::Move(3),
                    condition: Some(
                        "review finds gaps or user requests document changes".to_string(),
                    ),
                    output_to_choreo_memory: None,
                }],
            },
        ],
        description: "Creates or updates decision-alignment documents (specs, PRDs, RFCs, ADRs, \
            design docs) to help a reader evaluate or align on a decision."
            .to_string(),
    }
}
