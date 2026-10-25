use crate::tools::{ToolCore, ToolCoreCall};
use crate::utils::{is_within_directory, normalize_glob, path_from_str};
use globset::GlobBuilder;
use ignore::WalkBuilder;

use rig::tool::{ToolContext, ToolExecutionError};
use serde::{Deserialize, Serialize};
use serde_json::json;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ListFilesArgs {
    pub pattern: String,
    #[serde(default)]
    pub path: Option<String>,
    #[serde(default)]
    pub show_gitignored: Option<bool>,
    #[serde(default)]
    pub max_count: Option<usize>,
}

#[derive(Deserialize, Serialize, Clone, Default)]
pub struct ListFiles {
    /// Base directory for resolving `path`. `None` resolves against the process cwd.
    pub working_directory: Option<std::path::PathBuf>,
    /// When true, reject requests whose resolved path falls outside `working_directory`.
    pub restrict_to_working_directory: bool,
}

pub struct ListFilesCall {
    args: ListFilesArgs,
    working_directory: Option<std::path::PathBuf>,
    restrict_to_working_directory: bool,
}

impl ToolCoreCall for ListFilesCall {
    type Error = ToolExecutionError;
    type Output = String;

    async fn result(self, _context: &mut ToolContext) -> Result<Self::Output, Self::Error> {
        let args = self.args;
        let max_count = args.max_count.unwrap_or(20);
        let show_gitignored = args.show_gitignored.unwrap_or(false);
        let search_path = match &self.working_directory {
            Some(dir) => match args.path.as_deref() {
                None => dir.clone(),
                Some(p) => {
                    let path = path_from_str(p);
                    if path.is_relative() {
                        dir.join(path)
                    } else {
                        path
                    }
                }
            },
            None => args
                .path
                .as_deref()
                .map(path_from_str)
                .unwrap_or_else(|| path_from_str(".")),
        };
        if !search_path.exists() {
            return Err(ToolExecutionError::not_found(format!(
                "Directory '{}' not found",
                search_path.display()
            )));
        }
        if self.restrict_to_working_directory {
            // `None` restricts to the process cwd
            let base = match &self.working_directory {
                Some(dir) => dir.clone(),
                None => std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from(".")),
            };
            if !is_within_directory(&search_path, &base) {
                return Err(ToolExecutionError::invalid_args(format!(
                    "Path '{}' is outside the working directory '{}'",
                    search_path.display(),
                    base.display()
                )));
            }
        }

        let pattern = normalize_glob(&args.pattern);
        let glob = GlobBuilder::new(pattern)
            .literal_separator(true)
            .build()
            .map_err(|e| {
                ToolExecutionError::invalid_args(format!(
                    "Invalid glob pattern '{}': {}",
                    args.pattern, e
                ))
            })?
            .compile_matcher();

        let mut walker = WalkBuilder::new(&search_path);
        walker
            .git_ignore(!show_gitignored)
            .git_exclude(!show_gitignored)
            .git_global(!show_gitignored)
            .hidden(false)
            .follow_links(true)
            .require_git(true);

        let mut files: Vec<String> = Vec::new();
        let mut total_matched: usize = 0;

        for entry in walker.build() {
            match entry {
                Ok(e) => {
                    if !e.file_type().is_some_and(|ft| ft.is_file()) {
                        continue;
                    }
                    // Never list files inside .git directories
                    if e.path().components().any(|c| c.as_os_str() == ".git") {
                        continue;
                    }
                    let relative = e.path().strip_prefix(&search_path).unwrap_or(e.path());
                    if !glob.is_match(relative) {
                        continue;
                    }
                    total_matched += 1;
                    if files.len() < max_count {
                        // Explicit `path` -> full entry paths; default (working directory)
                        // -> paths relative to the search root
                        let out = if args.path.is_some() {
                            e.path()
                        } else {
                            relative
                        };
                        if let Some(out_str) = out.to_str() {
                            files.push(out_str.to_string());
                        }
                    }
                }
                Err(_) => continue,
            }
        }

        let truncated = total_matched > max_count;

        Ok(crate::utils::format_yaml_block_scalars(
            &serde_yaml::to_string(&json!({
                "files": files,
                "total_matched": total_matched,
                "truncated": truncated,
            }))
            .unwrap_or_else(|_| "files: []\ntotal_matched: 0\ntruncated: false".to_string()),
        ))
    }
}

impl ToolCore for ListFiles {
    fn name(&self) -> String {
        "list_files".to_string()
    }
    type Error = ToolExecutionError;
    type Args = ListFilesArgs;
    type Output = String;
    type Call = ListFilesCall;

    fn description(&self) -> String {
        "List files matching glob. YAML: files[] + metadata.".to_string()
    }

    fn parameters(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "pattern": {
                    "type": "string",
                    "description": "Glob pattern. `**/*.rs` matches recursively in all subdirs; `*.rs` matches files directly under path"
                },
                "path": {
                    "type": "string",
                    "description": "Search dir. Relative paths resolve against cwd; cwd if omitted"
                },
                "show_gitignored": {
                    "type": "boolean",
                    "description": "Include gitignored",
                    "default": false
                },
                "max_count": {
                    "type": "integer",
                    "description": "Max results",
                    "default": 20
                }
            },
            "required": ["pattern"]
        })
    }

    /// Read-only, no side effects
    fn properties(&self, _args: &ListFilesArgs) -> crate::chat::log::LogProperties {
        crate::chat::log::LogProperties {
            idempotent: true,
            important: false,
        }
    }

    async fn init_call(
        &self,
        _context: &mut ToolContext,
        args: Self::Args,
    ) -> Result<Self::Call, Self::Error> {
        Ok(ListFilesCall {
            args,
            working_directory: self.working_directory.clone(),
            restrict_to_working_directory: self.restrict_to_working_directory,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rig::tool::ToolContext;

    fn make_temp_tree(tag: &str, root_file: &str) -> std::path::PathBuf {
        let dir =
            std::env::temp_dir().join(format!("tenon_list_files_{}_{}", tag, std::process::id()));
        std::fs::create_dir_all(dir.join("sub")).unwrap();
        std::fs::write(dir.join(root_file), "").unwrap();
        std::fs::write(dir.join("sub").join("nested.txt"), "").unwrap();
        dir
    }

    async fn run_list(
        working_directory: Option<std::path::PathBuf>,
        restrict_to_working_directory: bool,
        path: Option<String>,
        pattern: &str,
    ) -> Result<String, ToolExecutionError> {
        let mut context = ToolContext::new();
        let call = ListFiles {
            working_directory,
            restrict_to_working_directory,
        }
        .init_call(
            &mut context,
            ListFilesArgs {
                pattern: pattern.to_string(),
                path,
                show_gitignored: None,
                max_count: None,
            },
        )
        .await
        .expect("init_call should succeed");
        call.result(&mut context).await
    }

    #[tokio::test]
    async fn test_path_resolution_against_working_directory() {
        let dir = make_temp_tree("wd", "root.txt");
        let other = make_temp_tree("other", "other_root.txt");

        // path None -> working directory, output relative to it
        let output = run_list(Some(dir.clone()), false, None, "*.txt")
            .await
            .expect("path None should resolve to working directory");
        assert!(output.contains("root.txt"), "got: {output}");
        assert!(!output.contains("nested.txt"), "got: {output}");
        assert!(!output.contains(dir.to_str().unwrap()), "got: {output}");

        // relative path -> joined against working directory, output full path
        let output = run_list(Some(dir.clone()), false, Some("sub".to_string()), "*.txt")
            .await
            .expect("relative path should resolve against working directory");
        let nested_full = dir.join("sub").join("nested.txt");
        assert!(
            output.contains(nested_full.to_str().unwrap()),
            "got: {output}"
        );
        assert!(!output.contains("root.txt"), "got: {output}");

        // absolute path -> used as-is, ignoring the working directory
        let output = run_list(
            Some(dir.clone()),
            false,
            Some(other.to_string_lossy().to_string()),
            "*.txt",
        )
        .await
        .expect("absolute path should be used as-is");
        assert!(output.contains("other_root.txt"), "got: {output}");
        assert!(!output.contains("nested.txt"), "got: {output}");

        std::fs::remove_dir_all(&dir).ok();
        std::fs::remove_dir_all(&other).ok();
    }

    #[tokio::test]
    async fn test_restrict_to_working_directory() {
        let dir = make_temp_tree("restrict", "root.txt");
        let other = make_temp_tree("restrict_other", "other_root.txt");

        // absolute path outside the working directory -> rejected
        let err = run_list(
            Some(dir.clone()),
            true,
            Some(other.to_string_lossy().to_string()),
            "*.txt",
        )
        .await
        .expect_err("absolute path outside working directory should be rejected");
        assert!(err.to_string().contains("outside the working directory"));

        // relative path escaping via `..` -> rejected
        let escape = format!("../{}", other.file_name().unwrap().to_string_lossy());
        let err = run_list(Some(dir.clone()), true, Some(escape), "*.txt")
            .await
            .expect_err("`..` escape should be rejected");
        assert!(err.to_string().contains("outside the working directory"));

        // relative path inside the working directory -> allowed
        let output = run_list(Some(dir.clone()), true, Some("sub".to_string()), "*.txt")
            .await
            .expect("relative path inside working directory should be allowed");
        assert!(output.contains("nested.txt"), "got: {output}");

        // no working directory configured -> restriction applies against cwd
        let err = run_list(
            None,
            true,
            Some(other.to_string_lossy().to_string()),
            "*.txt",
        )
        .await
        .expect_err("no working directory means restriction against cwd");
        assert!(err.to_string().contains("outside the working directory"));

        // path inside the cwd -> allowed
        let output = run_list(None, true, Some("src".to_string()), "*.rs")
            .await
            .expect("path inside cwd should be allowed");
        assert!(output.contains("src/lib.rs"), "got: {output}");

        std::fs::remove_dir_all(&dir).ok();
        std::fs::remove_dir_all(&other).ok();
    }
}
