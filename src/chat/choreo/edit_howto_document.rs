use super::{Choreo, GotoMove, Instruction, Move, MoveGotoInstruction, choreo_path};

pub fn choreo() -> Choreo {
    Choreo {
        id: "edit_howto_document".to_string(),
        title: "Edit How-to Document".to_string(),
        description: "Creates or updates a how-to guide - a task-oriented document guiding a competent user through a specific, real-world problem toward a result - grounded in the codebase and statically verified".to_string(),
        moves: vec![
            Move {
                title: "Understand Subject".to_string(),
                instruction: Instruction::File {
                    file: choreo_path("edit_howto_document/1_understand_subject.md"),
                },
                goto_instructions: vec![MoveGotoInstruction {
                    to: GotoMove::Next,
                    condition: None,
                    output_to_choreo_memory: Some("subject_context".to_string()),
                }],
            },
            Move {
                title: "Write How-to".to_string(),
                instruction: Instruction::File {
                    file: choreo_path("edit_howto_document/2_write_howto.md"),
                },
                goto_instructions: vec![],
            },
            Move {
                title: "Verify How-to".to_string(),
                instruction: Instruction::File {
                    file: choreo_path("edit_howto_document/3_verify_howto.md"),
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
                        condition: Some("user confirmed the document".to_string()),
                        output_to_choreo_memory: None,
                    },
                ],
            },
        ],
    }
}
