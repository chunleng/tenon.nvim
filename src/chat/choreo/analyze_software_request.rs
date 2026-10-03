use super::{Choreo, GotoMove, Instruction, Move, MoveGotoInstruction, choreo_path};

pub fn choreo() -> Choreo {
    Choreo {
        id: "analyze_software_request".to_string(),
        title: "Analyze Software Request".to_string(),
        moves: vec![
            Move {
                title: "Scope".to_string(),
                instruction: Instruction::File {
                    file: choreo_path("analyze_software_request/1_scope.md"),
                },
                goto_instructions: vec![MoveGotoInstruction {
                    to: GotoMove::Next,
                    condition: None,
                    output_to_choreo_memory: Some("scope".to_string()),
                }],
            },
            Move {
                title: "Plan".to_string(),
                instruction: Instruction::File {
                    file: choreo_path("analyze_software_request/2_plan.md"),
                },
                goto_instructions: vec![MoveGotoInstruction {
                    to: GotoMove::Next,
                    condition: None,
                    output_to_choreo_memory: Some("plan".to_string()),
                }],
            },
            Move {
                title: "Validate".to_string(),
                instruction: Instruction::File {
                    file: choreo_path("analyze_software_request/3_validate.md"),
                },
                goto_instructions: vec![
                    MoveGotoInstruction {
                        to: GotoMove::Move(1),
                        condition: Some("scope issues are found".to_string()),
                        output_to_choreo_memory: None,
                    },
                    MoveGotoInstruction {
                        to: GotoMove::Move(2),
                        condition: Some("plan issues are found".to_string()),
                        output_to_choreo_memory: None,
                    },
                ],
            },
        ],
        description: "Analyze a software request, whether creating new software or editing existing software: scope the request through codebase investigation and user interview, then plan an ordered sequence of non-breaking, user-visible milestone steps, and validate coverage against scope".to_string(),
    }
}
