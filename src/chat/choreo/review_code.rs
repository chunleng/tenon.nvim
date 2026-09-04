use super::{Choreo, GotoMove, Instruction, Move, MoveGotoInstruction, choreo_path};

pub fn choreo() -> Choreo {
    Choreo {
        id: "review_code".to_string(),
        title: "Review Code".to_string(),
        moves: vec![
            Move {
                title: "Setup Diff Source".to_string(),
                instruction: Instruction::File {
                    file: choreo_path("review_code/1_setup_diff_source.md"),
                },
                goto_instructions: vec![MoveGotoInstruction {
                    to: GotoMove::Next,
                    condition: None,
                    output_to_choreo_memory: Some("diff_source".to_string()),
                }],
            },
            Move {
                title: "Generate Findings".to_string(),
                instruction: Instruction::File {
                    file: choreo_path("review_code/2_generate_findings.md"),
                },
                goto_instructions: vec![
                    MoveGotoInstruction {
                        to: GotoMove::EndChoreo,
                        condition: Some(
                            "No findings were generated".to_string(),
                        ),
                        output_to_choreo_memory: None,
                    },
                    MoveGotoInstruction {
                        to: GotoMove::Next,
                        condition: None,
                        output_to_choreo_memory: Some("review_state".to_string()),
                    },
                ],
            },
            Move {
                title: "Filter & Report".to_string(),
                instruction: Instruction::File {
                    file: choreo_path("review_code/3_filter_and_report.md"),
                },
                goto_instructions: vec![
                    MoveGotoInstruction {
                        to: GotoMove::Move(2),
                        condition: Some(
                            "Any finding has been resolved via code changes".to_string(),
                        ),
                        output_to_choreo_memory: Some("review_state".to_string()),
                    },
                    MoveGotoInstruction {
                        to: GotoMove::EndChoreo,
                        condition: Some(
                            "All blockers have been resolved or dropped with no code changes".to_string(),
                        ),
                        output_to_choreo_memory: None,
                    },
                ],
            },
        ],
        description: "Review code changes against a diff, report blockers, and iterate with the author through fixes and pushback until LGTM".to_string(),
    }
}
