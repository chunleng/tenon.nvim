use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::OnceLock,
};

use serde::Deserialize;

use crate::utils::plugin_path;

#[derive(Debug, Clone, Deserialize)]
pub struct Knowledge {
    pub name: String,
    pub sources: Vec<KnowledgeSource>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase", deny_unknown_fields)]
pub enum KnowledgeSource {
    /// An inline string.
    Text { value: String },

    /// A file path. Relative paths are resolved against the current working directory.
    File { path: std::path::PathBuf },
}

pub static KNOWLEDGE_BASE: OnceLock<PathBuf> = OnceLock::new();

pub fn knowledge_path(relative: impl AsRef<Path>) -> PathBuf {
    KNOWLEDGE_BASE
        .get_or_init(|| plugin_path(PathBuf::from("markdown/knowledge")))
        .join(relative)
}

/// Returns the handcrafted system knowledge entries.
pub fn load_system_knowledge() -> HashMap<String, Knowledge> {
    let mut map = HashMap::new();

    map.insert(
        "AGENTS.md".into(),
        Knowledge {
            name: "AGENTS.md".into(),
            sources: vec![
                KnowledgeSource::File {
                    path: PathBuf::from("./AGENTS.md"),
                },
                KnowledgeSource::File {
                    path: PathBuf::from("./AGENTS.local.md"),
                },
            ],
        },
    );

    map.insert(
        "Caveman Mode".into(),
        Knowledge {
            name: "Caveman Mode".into(),
            sources: vec![KnowledgeSource::File {
                path: knowledge_path("caveman_mode.md"),
            }],
        },
    );

    map.insert(
        "Code Review Process".into(),
        Knowledge {
            name: "Code Review Process".into(),
            sources: vec![KnowledgeSource::File {
                path: knowledge_path("code_review_process.md"),
            }],
        },
    );

    map.insert(
        "Edit Prompt Process".into(),
        Knowledge {
            name: "Edit Prompt Process".into(),
            sources: vec![KnowledgeSource::File {
                path: knowledge_path("edit_prompt_process.md"),
            }],
        },
    );

    map.insert(
        "No Perfect Solution Attitude".into(),
        Knowledge {
            name: "No Perfect Solution Attitude".into(),
            sources: vec![KnowledgeSource::File {
                path: knowledge_path("no_perfect_solution_attitude.md"),
            }],
        },
    );

    map.insert(
        "Read First Attitude".into(),
        Knowledge {
            name: "Read First Attitude".into(),
            sources: vec![KnowledgeSource::File {
                path: knowledge_path("read_first_attitude.md"),
            }],
        },
    );

    map.insert(
        "YAGNI Attitude".into(),
        Knowledge {
            name: "YAGNI Attitude".into(),
            sources: vec![KnowledgeSource::File {
                path: knowledge_path("yagni_attitude.md"),
            }],
        },
    );

    map.insert(
        "Fix Software Bug Process".into(),
        Knowledge {
            name: "Fix Software Bug Process".into(),
            sources: vec![KnowledgeSource::File {
                path: knowledge_path("fix_software_bug_process.md"),
            }],
        },
    );

    map.insert(
        "Bug Isolation".into(),
        Knowledge {
            name: "Bug Isolation".into(),
            sources: vec![KnowledgeSource::File {
                path: knowledge_path("bug_isolation.md"),
            }],
        },
    );

    map.insert(
        "Prompt Editing Basics".into(),
        Knowledge {
            name: "Prompt Editing Basics".into(),
            sources: vec![KnowledgeSource::File {
                path: knowledge_path("prompt_editing_basics.md"),
            }],
        },
    );

    map.insert(
        "Prompting Basics".into(),
        Knowledge {
            name: "Prompting Basics".into(),
            sources: vec![KnowledgeSource::File {
                path: knowledge_path("prompting_basics.md"),
            }],
        },
    );

    map
}

impl Knowledge {
    /// Resolve all knowledge sources into a formatted knowledge block.
    ///
    /// Returns an XML-style block with clear boundaries:
    /// ```xml
    /// <knowledge name="knowledge-name">
    /// [content from source 1]
    ///
    /// ---
    ///
    /// [content from source 2]
    /// </knowledge>
    /// ```
    ///
    /// Filters out sources that resolve to None.
    pub fn resolve(&self) -> Result<String, std::io::Error> {
        let resolved: Result<Vec<_>, _> =
            self.sources.iter().map(|source| source.resolve()).collect();
        let resolved = resolved?;
        let non_empty: Vec<_> = resolved.into_iter().flatten().collect();
        let content = non_empty.join("\n\n---\n\n");

        Ok(format!(
            "<knowledge name=\"{}\">\n{}\n</knowledge>",
            self.name, content
        ))
    }
}

impl KnowledgeSource {
    /// Resolve the source into its final string content.
    ///
    /// Returns `Ok(None)` if the source has no content (empty text or missing/empty file).
    /// Returns `Ok(Some(content))` if the source has content.
    pub fn resolve(&self) -> Result<Option<String>, std::io::Error> {
        match self {
            KnowledgeSource::Text { value } => {
                if value.is_empty() {
                    Ok(None)
                } else {
                    Ok(Some(value.clone()))
                }
            }
            KnowledgeSource::File { path } => {
                let resolved = if path.is_absolute() {
                    path.clone()
                } else {
                    std::env::current_dir()?.join(path)
                };
                if resolved.exists() {
                    let content = std::fs::read_to_string(&resolved)?;
                    if content.is_empty() {
                        Ok(None)
                    } else {
                        Ok(Some(content))
                    }
                } else {
                    Ok(None)
                }
            }
        }
    }
}
