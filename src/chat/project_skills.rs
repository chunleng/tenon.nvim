use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::RwLock;

use crate::chat::choreo::{Choreo, ChoreoId, GotoMove, Instruction, Move, MoveGotoInstruction};
use crate::utils::{markdown::TenonMarkdown, path::TenonPath};
use anyhow::{Context, Result, ensure};
use heck::ToTitleCase;
use nvim_oxi::api::types::LogLevel;

const SKILL_SEARCH_PATHS: [&str; 2] = [".agents/skills", ".claude/skills"];

type ProjectSkills = std::sync::Arc<[Skill]>;

static SKILLS: RwLock<Option<(PathBuf, ProjectSkills)>> = RwLock::new(None);

#[derive(Clone)]
pub struct Skill {
    pub name: String,
    pub description: String,
    pub base_path: TenonPath,
}

impl Skill {
    fn new(base_path: TenonPath) -> Result<Self> {
        let root: PathBuf = base_path.clone().into();
        let path = root.join("SKILL.md");
        ensure!(fs::metadata(&path)?.is_file(), "expected a regular file");
        let markdown = TenonMarkdown::parse_file(&path)?;
        let metadata = markdown
            .frontmatter
            .as_object()
            .context("invalid YAML frontmatter")?;
        let metadata_name = metadata
            .get("name")
            .and_then(serde_json::Value::as_str)
            .context("name must be a string")?;
        ensure!(
            !metadata_name.is_empty()
                && metadata_name.chars().count() <= 64
                && !metadata_name.starts_with('-')
                && !metadata_name.ends_with('-')
                && !metadata_name.contains("--")
                && metadata_name == metadata_name.to_lowercase()
                && metadata_name
                    .chars()
                    .all(|c| c.is_alphanumeric() || c == '-'),
            "name must be 1 to 64 lowercase alphanumeric characters or single interior hyphens"
        );
        let description = metadata
            .get("description")
            .and_then(serde_json::Value::as_str)
            .context("description must be a string")?;
        ensure!(
            !description.trim().is_empty() && description.chars().count() <= 1024,
            "description must be non-blank and at most 1024 characters"
        );
        Ok(Self {
            name: metadata_name.to_string(),
            description: description.to_string(),
            base_path,
        })
    }
}

impl From<Skill> for Choreo {
    fn from(skill: Skill) -> Self {
        Self {
            id: ChoreoId::from(skill.name.as_str()).into(),
            title: skill.name.to_title_case(),
            description: skill.description,
            moves: vec![Move {
                title: "Run Skill".to_string(),
                instruction: Instruction::ProjectSkill {
                    base_path: skill.base_path,
                },
                goto_instructions: vec![MoveGotoInstruction {
                    to: GotoMove::EndChoreo,
                    condition: Some("skill complete".to_string()),
                    output_to_choreo_memory: None,
                }],
            }],
        }
    }
}

fn list_skill_entries(search_start: &Path) -> Vec<(&'static str, fs::DirEntry)> {
    let mut skill_entries = Vec::new();
    for search_path in SKILL_SEARCH_PATHS {
        let root = search_start.join(search_path);
        let entries = match fs::read_dir(&root) {
            Ok(entries) => entries,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                let broken_link =
                    [root.as_path(), root.parent().unwrap()]
                        .into_iter()
                        .find(|path| {
                            path.symlink_metadata()
                                .is_ok_and(|metadata| metadata.is_symlink())
                                && path.metadata().is_err()
                        });
                if let Some(path) = broken_link {
                    crate::utils::notify(
                        format!("[tenon] project skills: {}: {error}", path.display()),
                        LogLevel::Warn,
                    );
                }
                continue;
            }
            Err(error) => {
                crate::utils::notify(
                    format!("[tenon] project skills: {}: {error}", root.display()),
                    LogLevel::Warn,
                );
                continue;
            }
        };
        for entry in entries {
            match entry {
                Ok(entry) => skill_entries.push((search_path, entry)),
                Err(error) => crate::utils::notify(
                    format!("[tenon] project skills: {}: {error}", root.display()),
                    LogLevel::Warn,
                ),
            }
        }
    }
    skill_entries
}

pub fn cached() -> ProjectSkills {
    if let Ok(cwd) = std::env::current_dir()
        && let Some((_, skills)) = SKILLS
            .read()
            .unwrap()
            .as_ref()
            .filter(|(root, _)| *root == cwd)
    {
        return skills.clone();
    }
    discover()
}

pub fn discover() -> ProjectSkills {
    // TODO: Support agents whose working directory differs from the process cwd.
    let cwd = match std::env::current_dir() {
        Ok(cwd) => cwd,
        Err(error) => {
            crate::utils::notify(
                format!("[tenon] project skills: cannot get working directory: {error}"),
                LogLevel::Warn,
            );
            return Default::default();
        }
    };
    let mut skills = BTreeMap::new();
    for (search_path, entry) in list_skill_entries(&cwd) {
        let result = (|| -> Result<()> {
            let directory = entry.path();
            let metadata = directory
                .metadata()
                .with_context(|| format!("{}", directory.display()))?;
            if !metadata.is_dir() {
                return Ok(());
            }
            let file = directory.join("SKILL.md");
            let relative_path = Path::new(search_path).join(entry.file_name());
            let base_path = TenonPath::new(
                relative_path.to_str().with_context(|| {
                    format!("{}: skill folder path must be valid UTF-8", file.display())
                })?,
                Some(&cwd),
            );
            let skill = Skill::new(base_path).with_context(|| format!("{}", file.display()))?;
            skills.entry(skill.name.clone()).or_insert(skill);
            Ok(())
        })();
        if let Err(error) = result {
            let root = cwd.join(search_path);
            crate::utils::notify(
                format!("[tenon] project skills: {}: {error:#}", root.display()),
                LogLevel::Warn,
            );
        }
    }
    let skills: ProjectSkills = skills.into_values().collect();
    *SKILLS.write().unwrap() = Some((cwd, skills.clone()));
    skills
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Workspace(PathBuf);

    impl Workspace {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!("tenon-skills-{}", uuid::Uuid::new_v4()));
            fs::create_dir(&path).unwrap();
            Self(path)
        }

        fn write(&self, relative_path: &str, contents: impl AsRef<[u8]>) -> PathBuf {
            let path = self.0.join(relative_path);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(&path, contents).unwrap();
            path
        }
    }

    impl Drop for Workspace {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn lists_only_entries_in_the_working_directory_skill_roots() {
        let work = Workspace::new();
        for path in [
            ".agents/skills/local/SKILL.md",
            ".claude/skills/other/SKILL.md",
            ".agent/skills/wrong/SKILL.md",
            "nested/.agents/skills/child/SKILL.md",
        ] {
            work.write(path, "");
        }

        let mut entries: Vec<_> = list_skill_entries(&work.0)
            .into_iter()
            .map(|(search_path, entry)| {
                (
                    PathBuf::from(search_path),
                    entry.path().strip_prefix(&work.0).unwrap().to_owned(),
                )
            })
            .collect();
        entries.sort();

        assert_eq!(
            entries,
            [
                (
                    PathBuf::from(".agents/skills"),
                    PathBuf::from(".agents/skills/local")
                ),
                (
                    PathBuf::from(".claude/skills"),
                    PathBuf::from(".claude/skills/other")
                )
            ]
        );
    }

    #[cfg(unix)]
    #[test]
    fn skill_loading_follows_symlinks_and_preserves_listed_paths() {
        use std::os::unix::fs::symlink;

        let work = Workspace::new();
        let shared = Workspace::new();
        shared.write(
            "root/linked-root/SKILL.md",
            "---\nname: root-skill\ndescription: Linked root\n---\n",
        );
        shared.write(
            "directory/SKILL.md",
            "---\nname: linked-directory\ndescription: Linked directory\n---\n",
        );
        let file = shared.write(
            "file.md",
            "---\nname: linked-file\ndescription: Linked file\n---\n",
        );
        fs::create_dir(work.0.join(".agents")).unwrap();
        symlink(shared.0.join("root"), work.0.join(".agents/skills")).unwrap();
        symlink(
            shared.0.join("directory"),
            shared.0.join("root/linked-directory"),
        )
        .unwrap();
        fs::create_dir(shared.0.join("root/linked-file")).unwrap();
        symlink(file, shared.0.join("root/linked-file/SKILL.md")).unwrap();

        let mut skills: Vec<_> = list_skill_entries(&work.0)
            .into_iter()
            .map(|(_, entry)| {
                let directory = entry.path();
                let relative = directory.strip_prefix(&work.0).unwrap();
                let skill =
                    Skill::new(TenonPath::new(relative.to_str().unwrap(), Some(&work.0))).unwrap();
                assert_eq!(skill.base_path.as_ref(), relative);
                (skill.name.clone(), skill.base_path.as_ref().to_path_buf())
            })
            .collect();
        skills.sort();

        assert_eq!(
            skills,
            [
                (
                    "linked-directory".to_string(),
                    PathBuf::from(".agents/skills/linked-directory")
                ),
                (
                    "linked-file".to_string(),
                    PathBuf::from(".agents/skills/linked-file")
                ),
                (
                    "root-skill".to_string(),
                    PathBuf::from(".agents/skills/linked-root")
                ),
            ]
        );
    }

    #[tokio::test]
    async fn use_choreo_activates_without_requiring_the_skill_file() {
        use crate::chat::ActiveChoreo;
        use crate::tools::{
            ToolCore, ToolCoreCall,
            use_choreo::{UseChoreo, UseChoreoArgs},
        };
        use rig::tool::ToolContext;
        use std::sync::{Arc, RwLock};

        let work = Workspace::new();
        let relative = ".agents/skills/review/SKILL.md";
        let file = work.write(
            relative,
            "---\nname: code-review\ndescription: Review changes\n---\nOld body\n",
        );
        let skill = Skill::new(TenonPath::new(".agents/skills/review", Some(&work.0))).unwrap();
        let active = Arc::new(RwLock::new(None));
        let tool = UseChoreo {
            choreos: crate::tools::use_choreo::ChoreoCatalog {
                project: vec![skill].into(),
                ..Default::default()
            },
            active_choreo: active.clone(),
        };
        let mut context = ToolContext::new();

        assert_eq!(
            tool.parameters()["properties"]["choreo_id"]["enum"],
            serde_json::json!(["code_review"])
        );
        assert!(tool.description().contains("code_review: Review changes"));
        assert!(!tool.description().contains("Old body"));

        let current = "---\r\nname: code-review\r\ndescription: Review changes\r\n---\r\n<!-- note -->\r\nNew body\r\n";
        work.write(relative, current);
        let args = || UseChoreoArgs {
            choreo_id: "code_review".into(),
        };
        tool.init_call(&mut context, args())
            .await
            .unwrap()
            .result(&mut context)
            .await
            .unwrap();
        let ActiveChoreo { choreo, r#move, .. } = active.write().unwrap().take().unwrap();
        assert_eq!(r#move, 1);
        assert_eq!(choreo.title, "Code Review");
        assert_eq!(choreo.moves.len(), 1);
        assert_eq!(choreo.moves[0].instruction.resolve().unwrap(), current);

        fs::remove_file(file).unwrap();
        tool.init_call(&mut context, args())
            .await
            .unwrap()
            .result(&mut context)
            .await
            .unwrap();
        assert_eq!(
            active.read().unwrap().as_ref().unwrap().choreo.id,
            "code_review"
        );
    }

    #[test]
    fn validates_skill_frontmatter() {
        let work = Workspace::new();
        let read = |file: &Path| {
            let directory = file.parent().unwrap().strip_prefix(&work.0).unwrap();
            Skill::new(TenonPath::new(directory.to_str().unwrap(), Some(&work.0)))
        };
        for (folder, yaml) in [
            ("skill", "description: Missing name".to_string()),
            ("skill", "name: skill".to_string()),
            ("skill", "name: skill\ndescription: '   '".to_string()),
            ("skill", "name: skill\ndescription: 123".to_string()),
            ("123", "name: 123\ndescription: Numeric name".to_string()),
            ("Skill", "name: Skill\ndescription: Uppercase".to_string()),
            (
                "-skill",
                "name: -skill\ndescription: Leading hyphen".to_string(),
            ),
            (
                "skill-",
                "name: skill-\ndescription: Trailing hyphen".to_string(),
            ),
            (
                "some--skill",
                "name: some--skill\ndescription: Consecutive hyphens".to_string(),
            ),
            (
                "some_skill",
                "name: some_skill\ndescription: Underscore".to_string(),
            ),
            (
                "skill",
                format!("name: skill\ndescription: {}", "a".repeat(1025)),
            ),
            ("skill", "name: [broken\ndescription: Malformed".to_string()),
        ] {
            let file = work.write(
                &format!(".agents/skills/{folder}/SKILL.md"),
                format!("---\n{yaml}\n---\n"),
            );
            assert!(read(&file).is_err(), "accepted {yaml:?}");
        }
        let long_name = "a".repeat(65);
        let file = work.write(
            &format!(".agents/skills/{long_name}/SKILL.md"),
            format!("---\nname: {long_name}\ndescription: Long name\n---\n"),
        );
        assert!(read(&file).is_err());
        let file = work.write(
            ".agents/skills/different-folder/SKILL.md",
            format!(
                "---\nname: skill\ndescription: Valid\n# {}\n---\n{}",
                "a".repeat(65_537),
                "b".repeat(65_537)
            ),
        );
        let skill = read(&file).unwrap();
        assert_eq!(skill.name, "skill");
        assert_eq!(skill.description, "Valid");

        for contents in [
            "name: skill\ndescription: Missing frontmatter\n",
            "---\nname: skill\ndescription: Missing closing delimiter\n",
        ] {
            let file = work.write(".agents/skills/skill/SKILL.md", contents);
            assert!(read(&file).is_err(), "accepted {contents:?}");
        }
    }
}
