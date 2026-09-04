use super::{Choreo, GotoMove, Instruction, Move, MoveGotoInstruction, choreo_path};

pub fn choreo() -> Choreo {
    Choreo {
        id: "create_pr_description".to_string(),
        title: "Create PR Description".to_string(),
        moves: vec![
            Move {
                title: "Gather Diff".to_string(),
                instruction: Instruction::File {
                    file: choreo_path("create_pr_description/1_gather_diff.md"),
                },
                goto_instructions: vec![MoveGotoInstruction {
                    to: GotoMove::Next,
                    condition: None,
                    output_to_choreo_memory: Some("diff".to_string()),
                }],
            },
            Move {
                title: "Analyze Changes".to_string(),
                instruction: Instruction::File {
                    file: choreo_path("create_pr_description/2_analyze_changes.md"),
                },
                goto_instructions: vec![MoveGotoInstruction {
                    to: GotoMove::Next,
                    condition: None,
                    output_to_choreo_memory: Some("analysis".to_string()),
                }],
            },
            Move {
                title: "Generate Title & Description".to_string(),
                instruction: Instruction::File {
                    file: choreo_path("create_pr_description/3_generate.md"),
                },
                goto_instructions: vec![MoveGotoInstruction {
                    to: GotoMove::EndChoreo,
                    condition: None,
                    output_to_choreo_memory: None,
                }],
            },
        ],
        description: "Generates a PR title and description from code changes".to_string(),
    }
}
