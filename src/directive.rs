use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};

use crate::utils::plugin_path;

/// Describes where a directive's content comes from: an inline string, file paths,
/// or a registered preset file.
#[derive(Debug, Clone)]
pub enum DirectiveSource {
    /// An inline string. Easy for user to provide directly
    Text { value: String },

    /// File paths. Relative paths are resolved against the current working directory.
    /// Each file resolves into its own `<directive>` tag.
    File { paths: Vec<PathBuf> },

    /// A built-in preset file identified by a registry id.
    Preset { id: String, path: PathBuf },
}

impl DirectiveSource {
    /// Resolve the source into its final XML string format.
    ///
    /// - `Text` emits a single `<directive>` tag.
    /// - `File` emits one `<directive file="...">` tag per non-empty file.
    /// - `Preset` emits a single `<directive preset="...">` tag; a missing or
    ///   empty preset file returns an error (no tag is generated).
    ///
    /// Missing or empty files produce no tag. The condition, when present,
    /// is emitted as an attribute on every produced tag.
    pub fn resolve(&self, condition: Option<&str>) -> Result<String, std::io::Error> {
        let cond_attr = condition
            .map(|c| format!(r#" condition="{}""#, c))
            .unwrap_or_default();
        match self {
            DirectiveSource::Text { value } => {
                Ok(format!("<directive{}>{}</directive>", cond_attr, value))
            }
            DirectiveSource::File { paths } => {
                let mut tags = Vec::new();
                for path in paths {
                    if let Some(content) = read_non_empty(path)? {
                        tags.push(format!(
                            "<directive{} file=\"{}\">{}</directive>",
                            cond_attr,
                            path.display(),
                            content
                        ));
                    }
                }
                Ok(tags.join("\n"))
            }
            DirectiveSource::Preset { id, path } => {
                let Some(content) = read_non_empty(path)? else {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::NotFound,
                        format!("Preset file not found or empty: {}", path.display()),
                    ));
                };
                Ok(format!(
                    "<directive{} preset=\"{}\">{}</directive>",
                    cond_attr, id, content
                ))
            }
        }
    }
}

/// Reads a file, returning `None` when it is missing or empty.
/// Relative paths are resolved against the current working directory.
fn read_non_empty(path: &Path) -> Result<Option<String>, std::io::Error> {
    let resolved = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()?.join(path)
    };
    if !resolved.exists() {
        return Ok(None);
    }
    let content = std::fs::read_to_string(&resolved)?;
    if content.is_empty() {
        Ok(None)
    } else {
        Ok(Some(content))
    }
}

/// A directive with an optional condition for conditional application.
///
/// Wraps a `DirectiveSource` with an optional condition that determines
/// when the directive should be applied. When resolved, outputs XML format.
#[derive(Debug, Clone)]
pub struct Directive {
    /// Optional condition for conditional directive application.
    pub condition: Option<String>,
    /// The source of the directive content.
    pub source: DirectiveSource,
}

impl Directive {
    /// Resolve the directive into its final XML string format.
    ///
    /// Delegates to the source; the condition, when present, is emitted as an
    /// attribute on every produced tag.
    pub fn resolve(&self) -> Result<String, std::io::Error> {
        self.source.resolve(self.condition.as_deref())
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
        "No Perfect Solution Attitude".into(),
        Directive {
            condition: None,
            source: DirectiveSource::Preset {
                id: "No Perfect Solution Attitude".into(),
                path: directive_path("no_perfect_solution_attitude.md"),
            },
        },
    );

    map.insert(
        "Situation Sensitivity".into(),
        Directive {
            condition: None,
            source: DirectiveSource::Preset {
                id: "Situation Sensitivity".into(),
                path: directive_path("situation_sensitivity.md"),
            },
        },
    );

    map.insert(
        "Read First Attitude".into(),
        Directive {
            condition: None,
            source: DirectiveSource::Preset {
                id: "Read First Attitude".into(),
                path: directive_path("read_first_attitude.md"),
            },
        },
    );

    map.insert(
        "YAGNI Attitude".into(),
        Directive {
            condition: None,
            source: DirectiveSource::Preset {
                id: "YAGNI Attitude".into(),
                path: directive_path("yagni_attitude.md"),
            },
        },
    );

    map.insert(
        "Fix Software Bug Process".into(),
        Directive {
            condition: None,
            source: DirectiveSource::Preset {
                id: "Fix Software Bug Process".into(),
                path: directive_path("fix_software_bug_process.md"),
            },
        },
    );

    map.insert(
        "Bug Isolation".into(),
        Directive {
            condition: None,
            source: DirectiveSource::Preset {
                id: "Bug Isolation".into(),
                path: directive_path("bug_isolation.md"),
            },
        },
    );

    map.insert(
        "Prompt Editing Basics".into(),
        Directive {
            condition: None,
            source: DirectiveSource::Preset {
                id: "Prompt Editing Basics".into(),
                path: directive_path("prompt_editing_basics.md"),
            },
        },
    );

    map.insert(
        "Prompting Basics".into(),
        Directive {
            condition: None,
            source: DirectiveSource::Preset {
                id: "Prompting Basics".into(),
                path: directive_path("prompting_basics.md"),
            },
        },
    );

    map.insert(
        "Code Comment Basics".into(),
        Directive {
            condition: None,
            source: DirectiveSource::Preset {
                id: "Code Comment Basics".into(),
                path: directive_path("code_comment_basics.md"),
            },
        },
    );

    map.insert(
        "Testing Basics".into(),
        Directive {
            condition: None,
            source: DirectiveSource::Preset {
                id: "Testing Basics".into(),
                path: directive_path("testing_basics.md"),
            },
        },
    );

    map.insert(
        "Reduce Commentary".into(),
        Directive {
            condition: None,
            source: DirectiveSource::Preset {
                id: "Reduce Commentary".into(),
                path: directive_path("reduce-commentary.md"),
            },
        },
    );

    map.insert(
        "Speak With Facts".into(),
        Directive {
            condition: None,
            source: DirectiveSource::Preset {
                id: "Speak With Facts".into(),
                path: directive_path("speak_with_facts.md"),
            },
        },
    );

    map.insert(
        "Tenon Constitution".into(),
        Directive {
            condition: None,
            source: DirectiveSource::Preset {
                id: "Tenon Constitution".into(),
                path: directive_path("tenon_constitution.md"),
            },
        },
    );

    map
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_temp_file(name: &str, content: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "tenon_directive_{}_{}",
            name,
            std::process::id()
        ));
        std::fs::write(&path, content).unwrap();
        path
    }

    #[test]
    fn resolve_text_wraps_in_directive_tag() {
        let source = DirectiveSource::Text {
            value: "be careful".into(),
        };
        let resolved = source.resolve(None).unwrap();
        assert_eq!(resolved, "<directive>be careful</directive>");
    }

    #[test]
    fn resolve_text_includes_condition_attribute() {
        let source = DirectiveSource::Text {
            value: "be careful".into(),
        };
        let resolved = source.resolve(Some("when coding")).unwrap();
        assert_eq!(
            resolved,
            r#"<directive condition="when coding">be careful</directive>"#
        );
    }

    #[test]
    fn resolve_file_emits_one_tag_per_file() {
        let a = write_temp_file("file_a", "content a");
        let b = write_temp_file("file_b", "content b");
        let source = DirectiveSource::File {
            paths: vec![a.clone(), b.clone()],
        };
        let resolved = source.resolve(None).unwrap();
        assert_eq!(
            resolved,
            format!(
                "<directive file=\"{}\">content a</directive>\n<directive file=\"{}\">content b</directive>",
                a.display(),
                b.display()
            )
        );
    }

    #[test]
    fn resolve_file_skips_missing_and_empty_files() {
        let existing = write_temp_file("file_existing", "content");
        let empty = write_temp_file("file_empty", "");
        let missing = std::env::temp_dir().join("tenon_directive_missing_nonexistent");
        let source = DirectiveSource::File {
            paths: vec![missing, empty, existing.clone()],
        };
        let resolved = source.resolve(None).unwrap();
        assert_eq!(
            resolved,
            format!(
                "<directive file=\"{}\">content</directive>",
                existing.display()
            )
        );
    }

    #[test]
    fn resolve_preset_emits_id_tag() {
        let file = write_temp_file("preset_file", "preset content");
        let source = DirectiveSource::Preset {
            id: "YAGNI Attitude".into(),
            path: file,
        };
        let resolved = source.resolve(Some("when making code changes")).unwrap();
        assert_eq!(
            resolved,
            r#"<directive condition="when making code changes" preset="YAGNI Attitude">preset content</directive>"#
        );
    }

    #[test]
    fn resolve_preset_missing_file_returns_error() {
        let missing = std::env::temp_dir().join("tenon_directive_preset_missing_nonexistent");
        let source = DirectiveSource::Preset {
            id: "X".into(),
            path: missing.clone(),
        };
        let err = source.resolve(None).unwrap_err();
        assert_eq!(
            err.to_string(),
            format!("Preset file not found or empty: {}", missing.display())
        );
    }

    #[test]
    fn load_system_directives_agents_md_stays_file_others_preset() {
        let _ = crate::utils::PLUGIN_ROOT.set(PathBuf::from("."));
        let map = load_system_directives();
        assert!(matches!(
            &map["AGENTS.md"].source,
            DirectiveSource::File { .. }
        ));
        assert!(matches!(
            &map["YAGNI Attitude"].source,
            DirectiveSource::Preset { id, .. } if id == "YAGNI Attitude"
        ));
    }
}
