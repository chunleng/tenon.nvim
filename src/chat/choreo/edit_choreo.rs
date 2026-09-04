use super::{Choreo, GotoMove, Instruction, Move, MoveGotoInstruction, choreo_path};

pub fn choreo() -> Choreo {
    Choreo {
        id: "edit_choreo".to_string(),
        title: "Edit Choreo".to_string(),
        moves: vec![
            Move {
                title: "Setup".to_string(),
                instruction: Instruction::File {
                    file: choreo_path("edit_choreo/1_setup.md"),
                },
                goto_instructions: vec![MoveGotoInstruction {
                    to: GotoMove::Next,
                    condition: None,
                    output_to_choreo_memory: Some("setup".to_string()),
                }],
            },
            Move {
                title: "Gather Requirements".to_string(),
                instruction: Instruction::File {
                    file: choreo_path("edit_choreo/2_gather_requirements.md"),
                },
                goto_instructions: vec![MoveGotoInstruction {
                    to: GotoMove::Next,
                    condition: None,
                    output_to_choreo_memory: Some("requirements".to_string()),
                }],
            },
            Move {
                title: "Design Moves".to_string(),
                instruction: Instruction::File {
                    file: choreo_path("edit_choreo/3_design_moves.md"),
                },
                goto_instructions: vec![MoveGotoInstruction {
                    to: GotoMove::Next,
                    condition: None,
                    output_to_choreo_memory: Some("move_design".to_string()),
                }],
            },
            Move {
                title: "Draft".to_string(),
                instruction: Instruction::File {
                    file: choreo_path("edit_choreo/4_draft.md"),
                },
                goto_instructions: vec![
                    MoveGotoInstruction {
                        to: GotoMove::Move(3),
                        condition: Some("feedback involves move design changes".to_string()),
                        output_to_choreo_memory: None,
                    },
                ],
            },
            Move {
                title: "Review".to_string(),
                instruction: Instruction::File {
                    file: choreo_path("edit_choreo/5_review.md"),
                },
                goto_instructions: vec![
                    MoveGotoInstruction {
                        to: GotoMove::Move(3),
                        condition: Some("structural issues found".to_string()),
                        output_to_choreo_memory: None,
                    },
                    MoveGotoInstruction {
                        to: GotoMove::Move(4),
                        condition: Some("content issues found".to_string()),
                        output_to_choreo_memory: None,
                    },
                ],
            },
        ],
        description: "Create or update Tenon choreos through collaborative goal-setting, move isolation criteria, and impact-aware editing".to_string(),
    }
}
