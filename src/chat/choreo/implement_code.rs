use super::{Choreo, GotoMove, Instruction, Move, MoveGotoInstruction, choreo_path};

pub fn choreo() -> Choreo {
    Choreo {
        id: "implement_code".to_string(),
        title: "Implement Code".to_string(),
        moves: vec![
            Move {
                title: "Understand".to_string(),
                instruction: Instruction::File {
                    file: choreo_path("implement_code/1_understand.md"),
                },
                goto_instructions: vec![MoveGotoInstruction {
                    to: GotoMove::Next,
                    condition: None,
                    output_to_choreo_memory: Some("goal".to_string()),
                }],
            },
            Move {
                title: "Plan".to_string(),
                instruction: Instruction::File {
                    file: choreo_path("implement_code/2_plan.md"),
                },
                goto_instructions: vec![MoveGotoInstruction {
                    to: GotoMove::Next,
                    condition: None,
                    output_to_choreo_memory: Some("plan".to_string()),
                }],
            },
            Move {
                title: "Implement".to_string(),
                instruction: Instruction::File {
                    file: choreo_path("implement_code/3_implement.md"),
                },
                goto_instructions: vec![MoveGotoInstruction {
                    to: GotoMove::Next,
                    condition: Some(
                        "confirmed valid code and fixed error from linter/compiler".to_string()
                    ),
                    output_to_choreo_memory: None,
                }],
            },
            Move {
                title: "Verify".to_string(),
                instruction: Instruction::File {
                    file: choreo_path("implement_code/4_verify.md"),
                },
                goto_instructions: vec![
                    MoveGotoInstruction {
                        to: GotoMove::Move(3),
                        condition: Some("verification fails".to_string()),
                        output_to_choreo_memory: None,
                    },
                    MoveGotoInstruction {
                        to: GotoMove::Next,
                        condition: None,
                        output_to_choreo_memory: Some("unverifiable".to_string()),
                    },
                ],
            },
            Move {
                title: "Goal Check".to_string(),
                instruction: Instruction::File {
                    file: choreo_path("implement_code/5_goal_check.md"),
                },
                goto_instructions: vec![
                    MoveGotoInstruction {
                        to: GotoMove::Move(2),
                        condition: Some("goal not reached".to_string()),
                        output_to_choreo_memory: None,
                    },
                    MoveGotoInstruction {
                        to: GotoMove::Next,
                        condition: None,
                        output_to_choreo_memory: None,
                    },
                ],
            },
            Move {
                title: "Finalize".to_string(),
                instruction: Instruction::File {
                    file: choreo_path("implement_code/6_finalize.md"),
                },
                goto_instructions: vec![
                    MoveGotoInstruction {
                        to: GotoMove::Move(2),
                        condition: Some("tests fail".to_string()),
                        output_to_choreo_memory: None,
                    },
                    MoveGotoInstruction {
                        to: GotoMove::EndChoreo,
                        condition: None,
                        output_to_choreo_memory: None,
                    },
                ],
            },
        ],
        description: "Implements code changes through upfront planning, implement-then-verify cycles, and deviation-aware re-planning".to_string(),
    }
}
