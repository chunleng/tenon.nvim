use super::{Choreo, GotoMove, Instruction, Move, MoveGotoInstruction, choreo_path};

pub fn choreo() -> Choreo {
    Choreo {
        id: "edit_explanation_document".to_string(),
        title: "Edit Explanation Document".to_string(),
        moves: vec![
            Move {
                title: "Understand Subject".to_string(),
                instruction: Instruction::File {
                    file: choreo_path("edit_explanation_document/1_understand_subject.md"),
                },
                goto_instructions: vec![MoveGotoInstruction {
                    to: GotoMove::Next,
                    condition: None,
                    output_to_choreo_memory: Some("subject_context".to_string()),
                }],
            },
            Move {
                title: "Write Explanation".to_string(),
                instruction: Instruction::File {
                    file: choreo_path("edit_explanation_document/2_write_explanation.md"),
                },
                goto_instructions: vec![],
            },
            Move {
                title: "Verify Explanation".to_string(),
                instruction: Instruction::File {
                    file: choreo_path("edit_explanation_document/3_verify_explanation.md"),
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
        description: "Creates or updates an explanation document - an understanding-oriented document covering settled design decisions and their rationale, context, tradeoffs, and deliberate omissions - grounded in the codebase and verified for consistency".to_string(),
    }
}
