use globset::GlobBuilder;
use ignore::WalkBuilder;
use regex::RegexBuilder;
use rig::completion::ToolDefinition;
use rig::tool::{Tool, ToolError};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::path::Path;

const LINE_TRUNCATION_LIMIT: usize = 300;
const MATCH_LIMIT: usize = 100;

#[derive(Deserialize)]
pub struct SearchTextArgs {
    pub pattern: String,
    pub path: Option<String>,
    pub glob: Option<String>,
    pub is_regex: Option<bool>,
    pub ignore_case: Option<bool>,
    pub context_lines: Option<usize>,
    pub max_files: Option<usize>,
}

#[derive(Deserialize, Serialize, Clone)]
pub struct SearchText;

#[derive(Serialize)]
struct MatchEntry {
    line_number: usize,
    column_start: usize,
    column_end: usize,
    line: String,
    context_before: Vec<String>,
    context_after: Vec<String>,
}

#[derive(Serialize)]
struct FileEntry {
    path: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    matches: Vec<MatchEntry>,
}

#[derive(Serialize)]
struct SearchResult {
    files: Vec<FileEntry>,
    total_matches: usize,
    files_with_matches: usize,
    files_searched: usize,
    truncated_files: usize,
}

fn next_char_boundary(s: &str, pos: usize) -> usize {
    s[pos..]
        .char_indices()
        .nth(1)
        .map(|(i, _)| pos + i)
        .unwrap_or(s.len())
}

fn find_overlapping_matches(re: &regex::Regex, line: &str) -> Vec<(usize, usize)> {
    let mut matches = Vec::new();
    let mut pos = 0usize;
    while pos <= line.len() {
        match re.find_at(line, pos) {
            Some(m) => {
                if m.start() < pos {
                    pos = next_char_boundary(line, pos);
                    continue;
                }
                if m.start() == m.end() {
                    pos = next_char_boundary(line, m.start());
                    continue;
                }
                matches.push((m.start(), m.end()));
                pos = next_char_boundary(line, m.start());
            }
            None => break,
        }
    }
    matches
}

fn find_nonoverlapping_matches(re: &regex::Regex, line: &str) -> Vec<(usize, usize)> {
    re.find_iter(line).map(|m| (m.start(), m.end())).collect()
}

fn truncate_line(line: &str) -> String {
    if line.len() > LINE_TRUNCATION_LIMIT {
        let truncated: String = line.chars().take(LINE_TRUNCATION_LIMIT).collect();
        format!("{}…", truncated)
    } else {
        line.to_string()
    }
}

impl Tool for SearchText {
    const NAME: &'static str = "search_text";
    type Error = ToolError;
    type Args = SearchTextArgs;
    type Output = String;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: "search_text".to_string(),
            description: "Search text under directory. Returns match locations.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "pattern": {
                        "type": "string",
                        "description": "Text to find. Literal default. Set is_regex=true → regex"
                    },
                    "path": {
                        "type": "string",
                        "description": "Directory to search. Default=cwd"
                    },
                    "glob": {
                        "type": "string",
                        "description": "File filter. E.g. '*.rs', '**/*.ts'"
                    },
                    "is_regex": {
                        "type": "boolean",
                        "description": "Treat pattern as regex. Default=false"
                    },
                    "ignore_case": {
                        "type": "boolean",
                        "description": "Case-insensitive search. Default=false"
                    },
                    "context_lines": {
                        "type": "number",
                        "description": "Lines before+after match. Default=0"
                    },
                    "max_files": {
                        "type": "integer",
                        "description": "Max files returned. Default=all"
                    }
                },
                "required": ["pattern"]
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        let search_dir = args.path.unwrap_or_else(|| ".".to_string());
        let search_path = Path::new(&search_dir);

        if !search_path.exists() {
            return Err(ToolError::ToolCallError(Box::new(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("Directory '{}' not found", search_dir),
            ))));
        }

        let is_regex = args.is_regex.unwrap_or(false);
        let ignore_case = args.ignore_case.unwrap_or(false);
        let context_lines = args.context_lines.unwrap_or(0);
        let max_files = args.max_files;

        let pattern_str = if is_regex {
            args.pattern.clone()
        } else {
            regex::escape(&args.pattern)
        };

        let re = RegexBuilder::new(&pattern_str)
            .case_insensitive(ignore_case)
            .build()
            .map_err(|e| {
                ToolError::ToolCallError(Box::new(std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    format!("Invalid regex pattern '{}': {}", args.pattern, e),
                )))
            })?;

        let glob_matcher = args
            .glob
            .as_ref()
            .map(|g| {
                GlobBuilder::new(g)
                    .literal_separator(true)
                    .build()
                    .map_err(|e| {
                        ToolError::ToolCallError(Box::new(std::io::Error::new(
                            std::io::ErrorKind::InvalidInput,
                            format!("Invalid glob pattern '{}': {}", g, e),
                        )))
                    })
                    .map(|compiled| compiled.compile_matcher())
            })
            .transpose()?;

        let mut walker = WalkBuilder::new(search_path);
        walker
            .git_ignore(true)
            .git_exclude(true)
            .git_global(true)
            .hidden(false)
            .follow_links(true)
            .require_git(true);

        let mut file_results: Vec<FileEntry> = Vec::new();
        let mut files_searched: usize = 0;
        let mut total_matches: usize = 0;

        for entry in walker.build() {
            let entry = match entry {
                Ok(e) => e,
                Err(_) => continue,
            };

            if !entry.file_type().map_or(false, |ft| ft.is_file()) {
                continue;
            }

            if entry.path().components().any(|c| c.as_os_str() == ".git") {
                continue;
            }

            if let Some(ref matcher) = glob_matcher {
                let relative = entry
                    .path()
                    .strip_prefix(search_path)
                    .unwrap_or(entry.path());
                if !matcher.is_match(relative) {
                    continue;
                }
            }

            files_searched += 1;

            let content_bytes = match std::fs::read(entry.path()) {
                Ok(c) => c,
                Err(_) => continue,
            };

            // Skip binary files (contains null bytes)
            if content_bytes.contains(&0) {
                continue;
            }

            // Skip non-UTF-8 files
            let text = match String::from_utf8(content_bytes) {
                Ok(t) => t,
                Err(_) => continue,
            };

            let lines: Vec<&str> = text.lines().collect();
            let mut file_matches: Vec<MatchEntry> = Vec::new();

            for (line_idx, &line) in lines.iter().enumerate() {
                let span_matches = if is_regex {
                    find_nonoverlapping_matches(&re, line)
                } else {
                    find_overlapping_matches(&re, line)
                };

                for (col_start, col_end) in span_matches {
                    let line_number = line_idx + 1;

                    let context_before: Vec<String> = if context_lines > 0 {
                        let start = line_idx.saturating_sub(context_lines);
                        lines[start..line_idx]
                            .iter()
                            .map(|l| truncate_line(l))
                            .collect()
                    } else {
                        Vec::new()
                    };

                    let context_after: Vec<String> = if context_lines > 0 {
                        let end = (line_idx + 1 + context_lines).min(lines.len());
                        lines[line_idx + 1..end]
                            .iter()
                            .map(|l| truncate_line(l))
                            .collect()
                    } else {
                        Vec::new()
                    };

                    file_matches.push(MatchEntry {
                        line_number,
                        column_start: col_start,
                        column_end: col_end,
                        line: truncate_line(line),
                        context_before,
                        context_after,
                    });
                }
            }

            if !file_matches.is_empty() {
                total_matches += file_matches.len();
                let path_str = entry.path().to_str().unwrap_or_default().to_string();
                file_results.push(FileEntry {
                    path: path_str,
                    matches: file_matches,
                });
            }
        }

        // Sort by file path for deterministic order
        file_results.sort_by(|a, b| a.path.cmp(&b.path));

        // If total matches across all files exceeds limit, only show file paths
        let total_file_matches: usize = file_results.iter().map(|f| f.matches.len()).sum();
        if total_file_matches > MATCH_LIMIT {
            for entry in &mut file_results {
                entry.matches.clear();
            }
        }

        let files_with_matches = file_results.len();
        let truncated_files = if let Some(max) = max_files {
            let excess = file_results.len().saturating_sub(max);
            file_results.truncate(max);
            excess
        } else {
            0
        };

        let result = SearchResult {
            files: file_results,
            total_matches,
            files_with_matches,
            files_searched,
            truncated_files,
        };

        Ok(serde_json::to_string(&result).unwrap_or_else(|_| {
            r#"{"files":[],"total_matches":0,"files_with_matches":0,"files_searched":0,"truncated_files":0}"#
                .to_string()
        }))
    }
}
