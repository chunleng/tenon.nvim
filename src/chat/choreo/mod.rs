mod analyze_software_request;
mod assess_project_doc_needs;
mod create_pr_description;
mod edit_choreo;
mod edit_directive;
mod edit_explanation_document;
mod edit_howto_document;
mod edit_reference_document;
mod edit_software_specification;
mod edit_tutorial_document;
mod find_software_bug_root_cause;
mod implement_code;
mod implement_code_together;
mod plan_refactoring;
mod review_code;

use anyhow::{Result, anyhow};
use serde::Deserialize;
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};

use crate::chat::{TenonChoreoLog, TenonToolLog};
use crate::utils::plugin_path;

pub static CHOREO_BASE: OnceLock<std::path::PathBuf> = OnceLock::new();

pub fn choreo_path(relative: impl AsRef<Path>) -> std::path::PathBuf {
    CHOREO_BASE
        .get_or_init(|| plugin_path(std::path::PathBuf::from("markdown/choreo")))
        .join(relative)
}

/// Instruction content for a choreo move.
/// Can be inline text or a reference to a file.
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum Instruction {
    /// Inline text instruction
    Text(String),

    /// Path to a file containing the instruction
    File { file: PathBuf },

    // Project skills are discovered at runtime, not loaded from choreo definitions.
    #[serde(skip)]
    ProjectSkill {
        base_path: crate::utils::path::TenonPath,
    },
}

impl Instruction {
    /// Resolve the instruction to its final string content.
    pub fn resolve(&self) -> Result<String> {
        match self {
            Instruction::Text(text) => Ok(text.clone()),
            Instruction::File { file } => std::fs::read_to_string(file).map_err(|e| {
                anyhow!(
                    "Failed to read instruction file '{}': {}",
                    file.display(),
                    e
                )
            }),
            Instruction::ProjectSkill { base_path } => {
                let path = PathBuf::from(base_path.clone()).join("SKILL.md");
                let read_error = |error| {
                    anyhow!("Failed to read instruction file '{base_path}/SKILL.md': {error}")
                };
                if !path.metadata().map_err(read_error)?.is_file() {
                    return Err(anyhow!(
                        "Expected '{base_path}/SKILL.md' to be a regular file"
                    ));
                }
                std::fs::read_to_string(&path).map_err(read_error)
            }
        }
    }
}

impl Default for Instruction {
    fn default() -> Self {
        Instruction::Text(String::new())
    }
}

pub fn load_system_choreos() -> Vec<Arc<Choreo>> {
    vec![
        Arc::new(find_software_bug_root_cause::choreo()),
        Arc::new(create_pr_description::choreo()),
        Arc::new(edit_software_specification::choreo()),
        Arc::new(edit_choreo::choreo()),
        Arc::new(edit_directive::choreo()),
        Arc::new(implement_code::choreo()),
        Arc::new(plan_refactoring::choreo()),
        Arc::new(implement_code_together::choreo()),
        Arc::new(analyze_software_request::choreo()),
        Arc::new(assess_project_doc_needs::choreo()),
        Arc::new(edit_tutorial_document::choreo()),
        Arc::new(edit_reference_document::choreo()),
        Arc::new(edit_explanation_document::choreo()),
        Arc::new(edit_howto_document::choreo()),
        Arc::new(review_code::choreo()),
    ]
}

pub struct ChoreoId(String);

impl From<&str> for ChoreoId {
    fn from(name: &str) -> Self {
        use heck::ToSnakeCase;
        Self(name.to_snake_case())
    }
}

impl ChoreoId {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl From<ChoreoId> for String {
    fn from(id: ChoreoId) -> Self {
        id.0
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct Choreo {
    // TODO: Change this to use ChoreoId
    pub id: String,
    pub title: String,
    pub moves: Vec<Move>,
    pub description: String,
}

impl Choreo {
    pub fn is_project_skill(&self) -> bool {
        matches!(
            self.moves.as_slice(),
            [Move {
                instruction: Instruction::ProjectSkill { .. },
                ..
            }]
        )
    }

    pub fn generate_log(
        &self,
        move_number: usize,
        tool_log: TenonToolLog,
    ) -> Result<TenonChoreoLog> {
        Ok(TenonChoreoLog {
            id: self.id.clone(),
            content: format!(
                "{} - {} ({} of {})",
                self.title.clone(),
                self.moves
                    .get(move_number - 1)
                    .ok_or(anyhow!("invalid move number"))?
                    .title
                    .clone(),
                move_number,
                self.moves.len()
            ),
            r#move: Some(move_number),
            tool_log,
        })
    }
}

#[derive(Debug, Clone, Deserialize)]
pub enum GotoMove {
    Next,
    Move(usize),
    EndChoreo,
}

impl GotoMove {
    /// Resolves this goto target to a concrete move index.
    /// Returns `Some(move_index)` for `Next` (based on current_move) and `Move(n)`.
    /// Returns `None` for `EndChoreo` (not a move-based target).
    pub fn resolve_move_index(&self, current_move: usize) -> Option<usize> {
        match self {
            GotoMove::Next => Some(current_move + 1),
            GotoMove::Move(n) => Some(*n),
            GotoMove::EndChoreo => None,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct MoveGotoInstruction {
    pub to: GotoMove,
    pub condition: Option<String>,
    #[serde(default)]
    pub output_to_choreo_memory: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Move {
    pub title: String,
    #[serde(default)]
    pub instruction: Instruction,
    pub goto_instructions: Vec<MoveGotoInstruction>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::utils::path::TenonPath;

    #[test]
    fn identifies_only_single_move_project_skills() {
        let mut choreo = Choreo {
            id: "test".into(),
            title: "Test".into(),
            description: String::new(),
            moves: vec![],
        };

        assert!(!choreo.is_project_skill());
        choreo.moves.push(Move {
            title: "Run".into(),
            instruction: Instruction::Text("Native".into()),
            goto_instructions: vec![],
        });
        assert!(!choreo.is_project_skill());
        choreo.moves[0].instruction = Instruction::ProjectSkill {
            base_path: TenonPath::new(".agents/skills/test", None::<PathBuf>),
        };
        assert!(choreo.is_project_skill());
        choreo.moves.push(choreo.moves[0].clone());
        assert!(!choreo.is_project_skill());
    }
}
