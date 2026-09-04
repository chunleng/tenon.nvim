use super::{Choreo, GotoMove, Instruction, Move, MoveGotoInstruction, choreo_path};

pub fn choreo() -> Choreo {
    Choreo {
        id: "implement_code_together".to_string(),
        title: "Implement Code Together".to_string(),
        moves: vec![
            Move {
                title: "Listen for Next Goal".to_string(),
                instruction: Instruction::File {
                    file: choreo_path("implement_code_together/1_listen.md"),
                },
                goto_instructions: vec![
                    MoveGotoInstruction {
                        to: GotoMove::EndChoreo,
                        condition: Some("user says stop".to_string()),
                        output_to_choreo_memory: None,
                    },
                    MoveGotoInstruction {
                        to: GotoMove::Next,
                        condition: Some("show user latest goal and confirmed with user".to_string()),
                        output_to_choreo_memory: Some("goal".to_string()),
                    },
                ],
            },
            Move {
                title: "Prepare Test".to_string(),
                instruction: Instruction::File {
                    file: choreo_path("implement_code_together/2_prepare_test.md"),
                },
                goto_instructions: vec![MoveGotoInstruction {
                    to: GotoMove::Next,
                    condition: Some("test edited (if needed) and confirmed with user, or test skipped per directive condition".to_string()),
                    output_to_choreo_memory: None,
                }],
            },
            Move {
                title: "Implement".to_string(),
                instruction: Instruction::File {
                    file: choreo_path("implement_code_together/3_implement.md"),
                },
                goto_instructions: vec![MoveGotoInstruction {
                    to: GotoMove::Move(1),
                    condition: Some("user confirmed implementation".to_string()),
                    output_to_choreo_memory: None,
                }],
            },
        ],
        description: "Use when making changes to code. possible keywords: implement, rename, refactor, fix, move, extract, add, update, change, delete".to_string(),
    }
}
