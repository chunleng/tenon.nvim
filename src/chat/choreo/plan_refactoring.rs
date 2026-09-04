use super::{Choreo, GotoMove, Instruction, Move, MoveGotoInstruction, choreo_path};

pub fn choreo() -> Choreo {
    Choreo {
        id: "plan_refactoring".to_string(),
        title: "Plan Refactoring".to_string(),
        moves: vec![
            Move {
                title: "Set Goal".to_string(),
                instruction: Instruction::File {
                    file: choreo_path("plan_refactoring/1_set_goal.md"),
                },
                goto_instructions: vec![MoveGotoInstruction {
                    to: GotoMove::Next,
                    condition: None,
                    output_to_choreo_memory: Some("constraints".to_string()),
                }],
            },
            Move {
                title: "Plan".to_string(),
                instruction: Instruction::File {
                    file: choreo_path("plan_refactoring/2_plan.md"),
                },
                goto_instructions: vec![MoveGotoInstruction {
                    to: GotoMove::Next,
                    condition: None,
                    output_to_choreo_memory: None,
                }],
            },
            Move {
                title: "Prune".to_string(),
                instruction: Instruction::File {
                    file: choreo_path("plan_refactoring/3_prune.md"),
                },
                goto_instructions: vec![MoveGotoInstruction {
                    to: GotoMove::Next,
                    condition: None,
                    output_to_choreo_memory: None,
                }],
            },
            Move {
                title: "Verify".to_string(),
                instruction: Instruction::File {
                    file: choreo_path("plan_refactoring/4_verify.md"),
                },
                goto_instructions: vec![
                    MoveGotoInstruction {
                        to: GotoMove::Move(2),
                        condition: Some("verification failed".to_string()),
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
        description: "Plans code refactoring, does not execute code changes".to_string(),
    }
}
