use super::{Choreo, GotoMove, Instruction, Move, MoveGotoInstruction, choreo_path};

pub fn choreo() -> Choreo {
    Choreo {
        id: "edit_reference_document".to_string(),
        title: "Edit Reference Document".to_string(),
        moves: vec![
            Move {
                title: "Understand Subject".to_string(),
                instruction: Instruction::File {
                    file: choreo_path("edit_reference_document/1_understand_subject.md"),
                },
                goto_instructions: vec![
                    MoveGotoInstruction {
                        to: GotoMove::EndChoreo,
                        condition: Some(
                            "the target document is auto-generated".to_string(),
                        ),
                        output_to_choreo_memory: None,
                    },
                    MoveGotoInstruction {
                        to: GotoMove::Next,
                        condition: None,
                        output_to_choreo_memory: Some("subject_context".to_string()),
                    },
                ],
            },
            Move {
                title: "Write Reference".to_string(),
                instruction: Instruction::File {
                    file: choreo_path("edit_reference_document/2_write_reference.md"),
                },
                goto_instructions: vec![],
            },
            Move {
                title: "Verify Reference".to_string(),
                instruction: Instruction::File {
                    file: choreo_path("edit_reference_document/3_verify_reference.md"),
                },
                goto_instructions: vec![
                    MoveGotoInstruction {
                        to: GotoMove::Move(2),
                        condition: Some(
                            "issues found that require fixing the document, or user pointed out problems instead of confirming".to_string(),
                        ),
                        output_to_choreo_memory: None,
                    },
                    MoveGotoInstruction {
                        to: GotoMove::EndChoreo,
                        condition: Some("user confirmed the verification result".to_string()),
                        output_to_choreo_memory: None,
                    },
                ],
            },
        ],
        description: "Creates or updates a reference document - a factual lookup document (API, CLI, configuration, data model, etc.) - using per-kind templates, statically verified against the codebase".to_string(),
    }
}
