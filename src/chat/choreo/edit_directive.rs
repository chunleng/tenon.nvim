use super::{Choreo, GotoMove, Instruction, Move, MoveGotoInstruction, choreo_path};

pub fn choreo() -> Choreo {
    Choreo {
        id: "edit_directive".to_string(),
        title: "Edit Directive".to_string(),
        moves: vec![
            Move {
                title: "Investigate".to_string(),
                instruction: Instruction::File {
                    file: choreo_path("edit_directive/1_investigate.md"),
                },
                goto_instructions: vec![MoveGotoInstruction {
                    to: GotoMove::Next,
                    condition: None,
                    output_to_choreo_memory: Some("root_cause".to_string()),
                }],
            },
            Move {
                title: "Analyze".to_string(),
                instruction: Instruction::File {
                    file: choreo_path("edit_directive/2_analyze.md"),
                },
                goto_instructions: vec![
                    MoveGotoInstruction {
                        to: GotoMove::EndChoreo,
                        condition: Some("no directive needed".to_string()),
                        output_to_choreo_memory: None,
                    },
                    MoveGotoInstruction {
                        to: GotoMove::Next,
                        condition: None,
                        output_to_choreo_memory: Some("decision".to_string()),
                    },
                ],
            },
            Move {
                title: "Draft".to_string(),
                instruction: Instruction::File {
                    file: choreo_path("edit_directive/3_draft.md"),
                },
                goto_instructions: vec![],
            },
        ],
        description: "Create or update directives by investigating agent behavior problems, identifying root causes, and drafting targeted behavior-steering or knowledge-boosting rules".to_string(),
    }
}
