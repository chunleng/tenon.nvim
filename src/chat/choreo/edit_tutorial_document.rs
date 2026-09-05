use super::{Choreo, GotoMove, Instruction, Move, MoveGotoInstruction, choreo_path};

pub fn choreo() -> Choreo {
    Choreo {
        id: "edit_tutorial_document".to_string(),
        title: "Edit Tutorial Document".to_string(),
        moves: vec![
            Move {
                title: "Understand Subject".to_string(),
                instruction: Instruction::File {
                    file: choreo_path("edit_tutorial_document/1_understand_subject.md"),
                },
                goto_instructions: vec![MoveGotoInstruction {
                    to: GotoMove::Next,
                    condition: None,
                    output_to_choreo_memory: Some("subject_context".to_string()),
                }],
            },
            Move {
                title: "Write Tutorial".to_string(),
                instruction: Instruction::File {
                    file: choreo_path("edit_tutorial_document/2_write_tutorial.md"),
                },
                goto_instructions: vec![],
            },
            Move {
                title: "Verify Tutorial".to_string(),
                instruction: Instruction::File {
                    file: choreo_path("edit_tutorial_document/3_verify_tutorial.md"),
                },
                goto_instructions: vec![
                    MoveGotoInstruction {
                        to: GotoMove::Move(2),
                        condition: Some(
                            "issues found that require fixing the document, or the user pointed out problems instead of confirming".to_string(),
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
        description: "Creates or updates a tutorial document - a hands-on walkthrough for a newcomer - with mandatory sections (Goal, Prerequisites, Steps, Expected result), statically verified against the codebase".to_string(),
    }
}
