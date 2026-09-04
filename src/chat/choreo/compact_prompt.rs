use super::{Choreo, GotoMove, Instruction, Move, MoveGotoInstruction, choreo_path};

pub fn choreo() -> Choreo {
    Choreo {
        id: "compact_prompt".to_string(),
        title: "Compact Prompt".to_string(),
        moves: vec![
            Move {
                title: "Set Goal".to_string(),
                instruction: Instruction::File {
                    file: choreo_path("compact_prompt/1_set_goal.md"),
                },
                goto_instructions: vec![MoveGotoInstruction {
                    to: GotoMove::Next,
                    condition: None,
                    output_to_choreo_memory: Some("goal".to_string()),
                }],
            },
            Move {
                title: "Classify & Resolve".to_string(),
                instruction: Instruction::File {
                    file: choreo_path("compact_prompt/2_classify.md"),
                },
                goto_instructions: vec![MoveGotoInstruction {
                    to: GotoMove::Next,
                    condition: None,
                    output_to_choreo_memory: Some("classification".to_string()),
                }],
            },
            Move {
                title: "Compact".to_string(),
                instruction: Instruction::File {
                    file: choreo_path("compact_prompt/3_compact.md"),
                },
                goto_instructions: vec![],
            },
            Move {
                title: "Verify".to_string(),
                instruction: Instruction::File {
                    file: choreo_path("compact_prompt/4_verify.md"),
                },
                goto_instructions: vec![MoveGotoInstruction {
                    to: GotoMove::Move(3),
                    condition: Some("compaction issues found".to_string()),
                    output_to_choreo_memory: None,
                }],
            },
        ],
        description: "Compacts prompt and directive text while preserving meaning, conditions, and constraints".to_string(),
    }
}
