use crate::tools::{ToolCore, ToolCoreCall};
use crate::utils::{normalize_glob, path_from_str};
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

#[derive(Deserialize, Serialize, Clone)]
pub struct ListFiles;

pub struct ListFilesCall {
    args: ListFilesArgs,
}

impl ToolCoreCall for ListFilesCall {
    type Error = ToolExecutionError;
    type Output = String;

    async fn result(self, _context: &mut ToolContext) -> Result<Self::Output, Self::Error> {
        let args = self.args;
        let max_count = args.max_count.unwrap_or(20);
        let show_gitignored = args.show_gitignored.unwrap_or(false);
        let search_dir = args.path.unwrap_or_else(|| ".".to_string());
        let search_path = path_from_str(&search_dir);

        if !search_path.exists() {
            return Err(ToolExecutionError::not_found(format!(
                "Directory '{}' not found",
                search_dir
            )));
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
                    if files.len() < max_count
                        && let Some(path_str) = e.path().to_str()
                    {
                        files.push(path_str.to_string());
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
                    "description": "Search dir. cwd if omitted"
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

    async fn init_call(
        &self,
        _context: &mut ToolContext,
        args: Self::Args,
    ) -> Result<Self::Call, Self::Error> {
        Ok(ListFilesCall { args })
    }
}
