use super::{Choreo, GotoMove, Instruction, Move, MoveGotoInstruction, choreo_path};

pub fn choreo() -> Choreo {
    Choreo {
        id: "find_software_bug_root_cause".to_string(),
        title: "Find Software Bug Root Cause".to_string(),
        moves: vec![
            Move {
                title: "Define".to_string(),
                instruction: Instruction::File {
                    file: choreo_path("find_software_bug_root_cause/1_define.md"),
                },
                goto_instructions: vec![MoveGotoInstruction {
                    to: GotoMove::Next,
                    condition: None,
                    output_to_choreo_memory: None,
                }],
            },
            Move {
                title: "Locate".to_string(),
                instruction: Instruction::File {
                    file: choreo_path("find_software_bug_root_cause/2_locate.md"),
                },
                goto_instructions: vec![
                    MoveGotoInstruction {
                        to: GotoMove::Move(1),
                        condition: Some("unable to locate".to_string()),
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
                title: "Reproduce".to_string(),
                instruction: Instruction::File {
                    file: choreo_path("find_software_bug_root_cause/3_reproduce.md"),
                },
                goto_instructions: vec![
                    MoveGotoInstruction {
                        to: GotoMove::Move(1),
                        condition: Some("unable to create test".to_string()),
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
                title: "Conclude".to_string(),
                instruction: Instruction::File {
                    file: choreo_path("find_software_bug_root_cause/4_conclude.md"),
                },
                goto_instructions: vec![MoveGotoInstruction {
                    to: GotoMove::EndChoreo,
                    condition: None,
                    output_to_choreo_memory: None,
                }],
            },
        ],
        description: "Finds the root cause of a software bug through systematic investigation"
            .to_string(),
    }
}
