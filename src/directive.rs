use serde::Deserialize;
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};

use crate::utils::plugin_path;

/// Describes where a directive comes from: an inline string, file paths, or a system directive reference.
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase", deny_unknown_fields)]
pub enum DirectiveSource {
    /// An inline string. Easy for user to provide directly
    Text { value: String },

    /// File paths. Relative paths are resolved against the current working directory.
    File { paths: Vec<PathBuf> },

    /// Reference to a system directive by name.
    System { name: String },
}

impl DirectiveSource {
    /// Resolve the source into its final string content.
    ///
    /// For `Text` this returns the value directly.
    /// For `File` this reads all files and joins them with "---" separators.
    /// For `System` this looks up the system directive and resolves it.
    pub fn resolve(&self) -> Result<String, std::io::Error> {
        match self {
            DirectiveSource::Text { value } => Ok(value.clone()),
            DirectiveSource::File { paths } => {
                let resolved: Result<Vec<_>, std::io::Error> = paths
                    .iter()
                    .map(|path| {
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
                    })
                    .collect();
                let resolved = resolved?;
                let non_empty: Vec<_> = resolved.into_iter().flatten().collect();
                Ok(non_empty.join("\n\n---\n\n"))
            }
            DirectiveSource::System { name } => {
                let registry = crate::get_directive_registry();
                let directive = registry.get(name).ok_or_else(|| {
                    std::io::Error::new(
                        std::io::ErrorKind::NotFound,
                        format!("System directive '{}' not found", name),
                    )
                })?;
                directive.source.resolve()
            }
        }
    }
}

/// A directive with an optional condition for conditional application.
///
/// Wraps a `DirectiveSource` with an optional condition that determines
/// when the directive should be applied. When resolved, outputs XML format.
#[derive(Debug, Clone, Deserialize)]
pub struct Directive {
    /// Optional condition for conditional directive application.
    #[serde(default)]
    pub condition: Option<String>,
    /// The source of the directive content.
    #[serde(flatten)]
    pub source: DirectiveSource,
}

impl Directive {
    /// Resolve the directive into its final XML string format.
    ///
    /// - If condition is Some: `<directive condition="...">resolved_source</directive>`
    /// - If condition is None: `<directive>resolved_source</directive>`
    pub fn resolve(&self) -> Result<String, std::io::Error> {
        let source_content = self.source.resolve()?;
        match &self.condition {
            Some(cond) => Ok(format!(
                r#"<directive condition="{}">{}</directive>"#,
                cond, source_content
            )),
            None => Ok(format!("<directive>{}</directive>", source_content)),
        }
    }
}

pub static DIRECTIVE_BASE: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();

pub fn directive_path(relative: impl AsRef<Path>) -> PathBuf {
    DIRECTIVE_BASE
        .get_or_init(|| plugin_path(PathBuf::from("markdown/directive")))
        .join(relative)
}

/// Returns the handcrafted system directive entries.
pub fn load_system_directives() -> HashMap<String, Directive> {
    let mut map = HashMap::new();

    map.insert(
        "AGENTS.md".into(),
        Directive {
            condition: None,
            source: DirectiveSource::File {
                paths: vec![
                    PathBuf::from("./AGENTS.md"),
                    PathBuf::from("./AGENTS.local.md"),
                ],
            },
        },
    );

    map.insert(
        "Code Review Process".into(),
        Directive {
            condition: None,
            source: DirectiveSource::File {
                paths: vec![directive_path("code_review_process.md")],
            },
        },
    );

    map.insert(
        "Edit Prompt Process".into(),
        Directive {
            condition: None,
            source: DirectiveSource::File {
                paths: vec![directive_path("edit_prompt_process.md")],
            },
        },
    );

    map.insert(
        "No Perfect Solution Attitude".into(),
        Directive {
            condition: None,
            source: DirectiveSource::File {
                paths: vec![directive_path("no_perfect_solution_attitude.md")],
            },
        },
    );

    map.insert(
        "Read First Attitude".into(),
        Directive {
            condition: None,
            source: DirectiveSource::File {
                paths: vec![directive_path("read_first_attitude.md")],
            },
        },
    );

    map.insert(
        "YAGNI Attitude".into(),
        Directive {
            condition: None,
            source: DirectiveSource::File {
                paths: vec![directive_path("yagni_attitude.md")],
            },
        },
    );

    map.insert(
        "Fix Software Bug Process".into(),
        Directive {
            condition: None,
            source: DirectiveSource::File {
                paths: vec![directive_path("fix_software_bug_process.md")],
            },
        },
    );

    map.insert(
        "Bug Isolation".into(),
        Directive {
            condition: None,
            source: DirectiveSource::File {
                paths: vec![directive_path("bug_isolation.md")],
            },
        },
    );

    map.insert(
        "Prompt Editing Basics".into(),
        Directive {
            condition: None,
            source: DirectiveSource::File {
                paths: vec![directive_path("prompt_editing_basics.md")],
            },
        },
    );

    map.insert(
        "Prompting Basics".into(),
        Directive {
            condition: None,
            source: DirectiveSource::File {
                paths: vec![directive_path("prompting_basics.md")],
            },
        },
    );

    map.insert(
        "Code Comment Basics".into(),
        Directive {
            condition: None,
            source: DirectiveSource::File {
                paths: vec![directive_path("code_comment_basics.md")],
            },
        },
    );

    map.insert(
        "Testing Basics".into(),
        Directive {
            condition: None,
            source: DirectiveSource::File {
                paths: vec![directive_path("testing_basics.md")],
            },
        },
    );

    map.insert(
        "Reduce Commentary".into(),
        Directive {
            condition: None,
            source: DirectiveSource::File {
                paths: vec![directive_path("reduce-commentary.md")],
            },
        },
    );

    map
}
