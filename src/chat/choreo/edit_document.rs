use super::{Choreo, GotoMove, Instruction, Move, MoveGotoInstruction, choreo_path};

pub fn choreo() -> Choreo {
    Choreo {
        id: "edit_document".to_string(),
        title: "Edit Document".to_string(),
        moves: vec![
            Move {
                title: "Gather".to_string(),
                instruction: Instruction::File {
                    file: choreo_path("edit_document/1_gather.md"),
                },
                goto_instructions: vec![MoveGotoInstruction {
                    to: GotoMove::Next,
                    condition: None,
                    output_to_choreo_memory: None,
                }],
            },
            Move {
                title: "Set Goal".to_string(),
                instruction: Instruction::File {
                    file: choreo_path("edit_document/2_set_goal.md"),
                },
                goto_instructions: vec![MoveGotoInstruction {
                    to: GotoMove::Next,
                    condition: None,
                    output_to_choreo_memory: Some("goal".to_string()),
                }],
            },
            Move {
                title: "Execute".to_string(),
                instruction: Instruction::File {
                    file: choreo_path("edit_document/3_execute.md"),
                },
                goto_instructions: vec![MoveGotoInstruction {
                    to: GotoMove::Next,
                    condition: None,
                    output_to_choreo_memory: None,
                }],
            },
            Move {
                title: "Refine".to_string(),
                instruction: Instruction::File {
                    file: choreo_path("edit_document/4_refine.md"),
                },
                goto_instructions: vec![MoveGotoInstruction {
                    to: GotoMove::Next,
                    condition: None,
                    output_to_choreo_memory: None,
                }],
            },
            Move {
                title: "Check Goal".to_string(),
                instruction: Instruction::File {
                    file: choreo_path("edit_document/5_check_goal.md"),
                },
                goto_instructions: vec![
                    MoveGotoInstruction {
                        to: GotoMove::Move(3),
                        condition: Some("goal not achieved and iteration under 3".to_string()),
                        output_to_choreo_memory: Some("gaps".to_string()),
                    },
                    MoveGotoInstruction {
                        to: GotoMove::EndChoreo,
                        condition: None,
                        output_to_choreo_memory: None,
                    },
                ],
            },
        ],
        description:
            "Creates or updates documentation (e.g., README.md, doc folders, markdown files)"
                .to_string(),
    }
}
