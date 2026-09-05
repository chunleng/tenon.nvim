use super::{Choreo, Instruction, Move, choreo_path};

pub fn choreo() -> Choreo {
    Choreo {
        id: "assess_project_doc_needs".to_string(),
        title: "Assess Project Doc Needs".to_string(),
        moves: vec![
            Move {
                title: "Understand Change".to_string(),
                instruction: Instruction::File {
                    file: choreo_path("assess_project_doc_needs/1_understand_change.md"),
                },
                goto_instructions: vec![],
            },
            Move {
                title: "Assess Needs".to_string(),
                instruction: Instruction::File {
                    file: choreo_path("assess_project_doc_needs/2_assess_needs.md"),
                },
                goto_instructions: vec![],
            },
        ],
        description: "Determines what project documentation is needed to understand a development, and queues the documentation tasks".to_string(),
    }
}
