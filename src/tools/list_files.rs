use globset::GlobBuilder;
use ignore::WalkBuilder;
use rig::completion::ToolDefinition;
use rig::tool::{Tool, ToolError};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::path::Path;

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

impl Tool for ListFiles {
    const NAME: &'static str = "list_files";
    type Error = ToolError;
    type Args = ListFilesArgs;
    type Output = String;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: "list_files".to_string(),
            description: "List files matching glob. YAML: files[] + metadata.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "pattern": {
                        "type": "string",
                        "description": "Glob pattern. e.g '*.rs', '**/*.rs', 'src/*.py', 'test/**/test_*.py'"
                    },
                    "path": {
                        "type": "string",
                        "description": "Search dir. Default=cwd"
                    },
                    "show_gitignored": {
                        "type": "boolean",
                        "description": "Include gitignored. Default=false"
                    },
                    "max_count": {
                        "type": "integer",
                        "description": "Max results. Default=20"
                    }
                },
                "required": ["pattern"]
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        let max_count = args.max_count.unwrap_or(20);
        let show_gitignored = args.show_gitignored.unwrap_or(false);
        let search_dir = args.path.unwrap_or_else(|| ".".to_string());
        let search_path = Path::new(&search_dir);

        if !search_path.exists() {
            return Err(ToolError::ToolCallError(Box::new(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("Directory '{}' not found", search_dir),
            ))));
        }

        let glob = GlobBuilder::new(&args.pattern)
            .literal_separator(true)
            .build()
            .map_err(|e| {
                ToolError::ToolCallError(Box::new(std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    format!("Invalid glob pattern '{}': {}", args.pattern, e),
                )))
            })?
            .compile_matcher();

        let mut walker = WalkBuilder::new(search_path);
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
                    let relative = e.path().strip_prefix(search_path).unwrap_or(e.path());
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
