use crate::tools::{ToolCore, ToolCoreCall};
use crate::utils::{format_yaml_block_scalars, is_within_directory, normalize_glob, path_from_str};
use globset::GlobBuilder;
use ignore::WalkBuilder;
use regex::RegexBuilder;

use rig::tool::{ToolContext, ToolExecutionError};
use serde::{Deserialize, Serialize};
use serde_json::json;

const LINE_TRUNCATION_LIMIT: usize = 300;
const MATCH_LIMIT: usize = 100;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SearchTextArgs {
    pub pattern: Option<String>,
    pub literal: Option<String>,
    pub path: Option<String>,
    pub glob: Option<String>,
    pub ignore_case: Option<bool>,
    pub context_lines: Option<usize>,
    pub max_files: Option<usize>,
}

#[derive(Deserialize, Serialize, Clone, Default)]
pub struct SearchText {
    /// Base directory for resolving `path`. `None` resolves against the process cwd.
    pub working_directory: Option<std::path::PathBuf>,
    /// When true, reject requests whose resolved path falls outside `working_directory`.
    pub restrict_to_working_directory: bool,
}

#[derive(Serialize)]
struct MatchEntry {
    line_number: usize,
    line: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    context_before: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
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
    total_line_matches: usize,
    files_with_matches: usize,
    files_searched: usize,
    truncated_files: usize,
}

fn truncate_line(line: &str) -> String {
    if line.chars().count() > LINE_TRUNCATION_LIMIT {
        let truncated: String = line.chars().take(LINE_TRUNCATION_LIMIT).collect();
        format!("{}…", truncated)
    } else {
        line.to_string()
    }
}

pub struct SearchTextCall {
    args: SearchTextArgs,
    working_directory: Option<std::path::PathBuf>,
    restrict_to_working_directory: bool,
}

impl ToolCoreCall for SearchTextCall {
    type Error = ToolExecutionError;
    type Output = String;

    async fn result(self, _context: &mut ToolContext) -> Result<Self::Output, Self::Error> {
        let args = self.args;
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

        let ignore_case = args.ignore_case.unwrap_or(false);
        let context_lines = args.context_lines.unwrap_or(0);
        let max_files = args.max_files;

        let (pattern_text, is_regex) = match (args.pattern.as_deref(), args.literal.as_deref()) {
            (Some(p), Some(l)) => match (!p.is_empty(), !l.is_empty()) {
                (true, false) => (p, true),
                (false, true) => (l, false),
                // Both empty -> use literal
                (false, false) => (l, false),
                (true, true) => {
                    return Err(ToolExecutionError::invalid_args(
                        "Provide exactly one of `literal` or `pattern`".to_string(),
                    ));
                }
            },
            (Some(p), None) => (p, true),
            (None, Some(l)) => (l, false),
            (None, None) => {
                return Err(ToolExecutionError::invalid_args(
                    "Provide exactly one of `literal` or `pattern`".to_string(),
                ));
            }
        };

        let pattern_str = if is_regex {
            pattern_text.to_string()
        } else {
            regex::escape(pattern_text)
        };

        let re = RegexBuilder::new(&pattern_str)
            .case_insensitive(ignore_case)
            .build()
            .map_err(|e| {
                ToolExecutionError::invalid_args(format!(
                    "Invalid regex pattern '{}': {}",
                    pattern_text, e
                ))
            })?;

        let glob_matcher = args
            .glob
            .as_ref()
            .map(|g| {
                let normalized = normalize_glob(g);
                GlobBuilder::new(normalized)
                    .literal_separator(true)
                    .build()
                    .map_err(|e| {
                        ToolExecutionError::invalid_args(format!(
                            "Invalid glob pattern '{}': {}",
                            g, e
                        ))
                    })
                    .map(|compiled| compiled.compile_matcher())
            })
            .transpose()?;

        let mut walker = WalkBuilder::new(&search_path);
        walker
            .git_ignore(true)
            .git_exclude(true)
            .git_global(true)
            .hidden(false)
            .follow_links(true)
            .require_git(true);

        let mut files = Vec::new();

        for entry in walker.build() {
            let entry = match entry {
                Ok(e) => e,
                Err(_) => continue,
            };

            if !entry.file_type().is_some_and(|ft| ft.is_file()) {
                continue;
            }

            if entry.path().components().any(|c| c.as_os_str() == ".git") {
                continue;
            }

            let relative = entry
                .path()
                .strip_prefix(&search_path)
                .unwrap_or(entry.path());

            if let Some(ref matcher) = glob_matcher {
                if !matcher.is_match(relative) {
                    continue;
                }
            }

            let content_bytes = match std::fs::read(entry.path()) {
                Ok(c) => c,
                Err(_) => continue,
            };

            if content_bytes.contains(&0) {
                continue;
            }

            let text = match String::from_utf8(content_bytes) {
                Ok(t) => t,
                Err(_) => continue,
            };

            // Explicit `path` -> full entry paths; default (working directory)
            // -> paths relative to the search root
            let out = if args.path.is_some() {
                entry.path()
            } else {
                relative
            };
            let path_str = out.to_str().unwrap_or_default().to_string();
            files.push((path_str, text));
        }

        let result = perform_search(files, &re, context_lines, max_files);

        Ok(format_yaml_block_scalars(
            &serde_yaml::to_string(&result).unwrap_or_else(|_| {
                "files: []\ntotal_line_matches: 0\nfiles_with_matches: 0\nfiles_searched: 0\ntruncated_files: 0\n"
                    .to_string()
            }),
        ))
    }
}

impl ToolCore for SearchText {
    fn name(&self) -> String {
        "search_text".to_string()
    }
    type Error = ToolExecutionError;
    type Args = SearchTextArgs;
    type Output = String;
    type Call = SearchTextCall;

    fn description(&self) -> String {
        "Search text under directory. Must provide exactly one of `pattern`/`literal`. Returns match locations".to_string()
    }

    fn parameters(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "pattern": {
                    "type": "string",
                    "default": null,
                    "description": "Regex pattern to find, e.g. alternation (`a|b`)"
                },
                "literal": {
                    "type": "string",
                    "default": null,
                    "description": "Literal text to find (exact match, no regex interpretation)"
                },
                "path": {
                    "type": "string",
                    "description": "Directory to search. Relative paths resolve against cwd; cwd if omitted"
                },
                "glob": {
                    "type": "string",
                    "description": "File filter. `**/*.rs` matches recursively in all subdirs; `*.rs` matches files directly under path. All files if omitted"
                },
                "ignore_case": {
                    "type": "boolean",
                    "description": "Case-insensitive search",
                    "default": false
                },
                "context_lines": {
                    "type": "number",
                    "description": "Lines before+after match",
                    "default": 0
                },
                "max_files": {
                    "type": "integer",
                    "description": "Max files returned. All files if omitted"
                }
            },
        })
    }

    /// Read-only, no side effects
    fn properties(&self, _args: &SearchTextArgs) -> crate::chat::log::LogProperties {
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
        Ok(SearchTextCall {
            args,
            working_directory: self.working_directory.clone(),
            restrict_to_working_directory: self.restrict_to_working_directory,
        })
    }
}

fn perform_search(
    files: Vec<(String, String)>,
    re: &regex::Regex,
    context_lines: usize,
    max_files: Option<usize>,
) -> SearchResult {
    let mut file_results: Vec<FileEntry> = Vec::new();
    let mut total_line_matches: usize = 0;
    let files_searched = files.len();

    for (path, text) in files {
        let lines: Vec<&str> = text.lines().collect();
        let mut file_matches: Vec<MatchEntry> = Vec::new();

        for (line_idx, &line) in lines.iter().enumerate() {
            if !re.is_match(line) {
                continue;
            }

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
                line: truncate_line(line),
                context_before,
                context_after,
            });
        }

        if !file_matches.is_empty() {
            total_line_matches += file_matches.len();
            file_results.push(FileEntry {
                path,
                matches: file_matches,
            });
        }
    }

    file_results.sort_by(|a, b| a.path.cmp(&b.path));

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

    SearchResult {
        files: file_results,
        total_line_matches,
        files_with_matches,
        files_searched,
        truncated_files,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    mod perform_search_tests {
        use super::*;

        #[test]
        fn basic_search() {
            let files = vec![
                ("file1.txt".to_string(), "hello world\nfoo bar".to_string()),
                ("file2.txt".to_string(), "hello rust\nfoo baz".to_string()),
            ];
            let re = regex::Regex::new("hello").unwrap();
            let result = perform_search(files, &re, 0, None);

            assert_eq!(result.total_line_matches, 2);
            assert_eq!(result.files_with_matches, 2);
            assert_eq!(result.files[0].path, "file1.txt");
            assert_eq!(result.files[0].matches[0].line, "hello world");
        }

        #[test]
        fn regex_search() {
            let files = vec![("file1.txt".to_string(), "123 hello\n456 world".to_string())];
            let re = regex::Regex::new(r"\d+").unwrap();
            let result = perform_search(files, &re, 0, None);

            assert_eq!(result.total_line_matches, 2);
            assert_eq!(result.files[0].matches[0].line, "123 hello");
        }

        #[test]
        fn context_lines() {
            let files = vec![(
                "file1.txt".to_string(),
                "line1\nline2\nline3\nline4\nline5".to_string(),
            )];
            let re = regex::Regex::new("line3").unwrap();
            let result = perform_search(files, &re, 1, None);

            let match_entry = &result.files[0].matches[0];
            assert_eq!(match_entry.context_before, vec!["line2".to_string()]);
            assert_eq!(match_entry.context_after, vec!["line4".to_string()]);
        }

        #[test]
        fn max_files_truncation() {
            let files = vec![
                ("a.txt".to_string(), "match".to_string()),
                ("b.txt".to_string(), "match".to_string()),
                ("c.txt".to_string(), "match".to_string()),
            ];
            let re = regex::Regex::new("match").unwrap();
            let result = perform_search(files, &re, 0, Some(2));

            assert_eq!(result.files.len(), 2);
            assert_eq!(result.truncated_files, 1);
        }

        #[test]
        fn match_limit_clears_matches() {
            let files = vec![
                ("file1.txt".to_string(), "a\n".repeat(120).to_string()), // > 100 matched lines
            ];
            let re = regex::Regex::new("a").unwrap();
            let result = perform_search(files, &re, 0, None);

            assert!(result.total_line_matches > MATCH_LIMIT);
            assert!(result.files[0].matches.is_empty());
        }
    }

    mod truncate_line {
        use super::*;

        #[test]
        fn short_line_unchanged() {
            let line = "short line";
            assert_eq!(truncate_line(line), line);
        }

        #[test]
        fn exactly_at_limit() {
            let line: String = "x".repeat(300);
            assert_eq!(truncate_line(&line), line);
            assert!(!truncate_line(&line).ends_with('…'));
        }

        #[test]
        fn over_limit_truncated() {
            let line: String = "x".repeat(301);
            let result = truncate_line(&line);
            // 300 ASCII chars + '…' (3 bytes) = 303 bytes total
            assert_eq!(result.len(), 303);
            assert!(result.ends_with('…'));
            // But only 301 characters (300 'x' + 1 '…')
            assert_eq!(result.chars().count(), 301);
        }

        #[test]
        fn multibyte_char_truncation() {
            // Each '日' is 3 bytes, but chars().take() counts characters
            // Create a line with enough multibyte chars to exceed byte limit AND char limit
            let line: String = "日".repeat(400); // 1200 bytes, 400 chars
            let result = truncate_line(&line);
            // Truncates to 300 chars (900 bytes), adds '…' (3 bytes) = 903 bytes
            assert_eq!(result.chars().count(), 301); // 300 '日' + '…'
            assert!(result.ends_with('…'));
        }

        #[test]
        fn no_truncation_when_under_char_limit() {
            // 101 characters, 303 bytes.
            // Should NOT be truncated because it's only 101 characters.
            let line: String = "日".repeat(101);
            let result = truncate_line(&line);
            assert_eq!(result, line);
            assert!(!result.ends_with('…'));
        }

        #[test]
        fn empty_line() {
            assert_eq!(truncate_line(""), "");
        }
    }

    mod search_result_serialization {
        use super::*;

        #[test]
        fn empty_result() {
            let result = SearchResult {
                files: vec![],
                total_line_matches: 0,
                files_with_matches: 0,
                files_searched: 0,
                truncated_files: 0,
            };
            let json = serde_json::to_string(&result).unwrap();
            assert!(json.contains("\"files\":[]"));
            assert!(json.contains("\"total_line_matches\":0"));
        }

        #[test]
        fn result_with_matches() {
            let result = SearchResult {
                files: vec![FileEntry {
                    path: "test.rs".to_string(),
                    matches: vec![MatchEntry {
                        line_number: 1,
                        line: "test".to_string(),
                        context_before: vec![],
                        context_after: vec!["next".to_string()],
                    }],
                }],
                total_line_matches: 1,
                files_with_matches: 1,
                files_searched: 5,
                truncated_files: 0,
            };
            let json = serde_json::to_string(&result).unwrap();
            let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();

            assert_eq!(parsed["total_line_matches"], 1);
            assert_eq!(parsed["files_searched"], 5);
            assert_eq!(parsed["files"][0]["path"], "test.rs");
            assert_eq!(parsed["files"][0]["matches"][0]["line_number"], 1);
        }

        #[test]
        fn file_entry_empty_matches_skipped() {
            let entry = FileEntry {
                path: "test.rs".to_string(),
                matches: vec![],
            };
            let json = serde_json::to_string(&entry).unwrap();
            // Empty matches array is skipped due to #[serde(skip_serializing_if)]
            assert!(!json.contains("matches"));
            assert!(json.contains("\"path\":\"test.rs\""));
        }

        #[test]
        fn context_lines() {
            let entry = MatchEntry {
                line_number: 5,
                line: "foo".to_string(),
                context_before: vec!["line3".to_string(), "line4".to_string()],
                context_after: vec!["line6".to_string()],
            };
            let json = serde_json::to_string(&entry).unwrap();
            let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();

            assert_eq!(parsed["context_before"], json!(["line3", "line4"]));
            assert_eq!(parsed["context_after"], json!(["line6"]));
        }
    }

    mod path_resolution {
        use super::*;
        use rig::tool::ToolContext;

        fn make_temp_tree(tag: &str, root_file: &str) -> std::path::PathBuf {
            let dir = std::env::temp_dir().join(format!(
                "tenon_search_text_{}_{}",
                tag,
                std::process::id()
            ));
            std::fs::create_dir_all(dir.join("sub")).unwrap();
            std::fs::write(dir.join(root_file), "needle here").unwrap();
            std::fs::write(dir.join("sub").join("nested.txt"), "needle here").unwrap();
            dir
        }

        async fn run_search(
            working_directory: Option<std::path::PathBuf>,
            restrict_to_working_directory: bool,
            path: Option<String>,
        ) -> Result<String, ToolExecutionError> {
            let mut context = ToolContext::new();
            let call = SearchText {
                working_directory,
                restrict_to_working_directory,
            }
            .init_call(
                &mut context,
                SearchTextArgs {
                    pattern: None,
                    literal: Some("needle".to_string()),
                    path,
                    glob: None,
                    ignore_case: None,
                    context_lines: None,
                    max_files: None,
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
            let output = run_search(Some(dir.clone()), false, None)
                .await
                .expect("path None should resolve to working directory");
            assert!(output.contains("root.txt"), "got: {output}");
            assert!(output.contains("nested.txt"), "got: {output}");
            assert!(!output.contains(dir.to_str().unwrap()), "got: {output}");

            // relative path -> joined against working directory, output full path
            let output = run_search(Some(dir.clone()), false, Some("sub".to_string()))
                .await
                .expect("relative path should resolve against working directory");
            let nested_full = dir.join("sub").join("nested.txt");
            assert!(
                output.contains(nested_full.to_str().unwrap()),
                "got: {output}"
            );
            assert!(!output.contains("root.txt"), "got: {output}");

            // absolute path -> used as-is, ignoring the working directory
            let output = run_search(
                Some(dir.clone()),
                false,
                Some(other.to_string_lossy().to_string()),
            )
            .await
            .expect("absolute path should be used as-is");
            assert!(output.contains("other_root.txt"), "got: {output}");
            assert!(!output.contains(dir.to_str().unwrap()), "got: {output}");

            std::fs::remove_dir_all(&dir).ok();
            std::fs::remove_dir_all(&other).ok();
        }

        #[tokio::test]
        async fn test_restrict_to_working_directory() {
            let dir = make_temp_tree("restrict", "root.txt");
            let other = make_temp_tree("restrict_other", "other_root.txt");

            // absolute path outside the working directory -> rejected
            let err = run_search(
                Some(dir.clone()),
                true,
                Some(other.to_string_lossy().to_string()),
            )
            .await
            .expect_err("absolute path outside working directory should be rejected");
            assert!(err.to_string().contains("outside the working directory"));

            // relative path escaping via `..` -> rejected
            let escape = format!("../{}", other.file_name().unwrap().to_string_lossy());
            let err = run_search(Some(dir.clone()), true, Some(escape))
                .await
                .expect_err("`..` escape should be rejected");
            assert!(err.to_string().contains("outside the working directory"));

            // relative path inside the working directory -> allowed
            let output = run_search(Some(dir.clone()), true, Some("sub".to_string()))
                .await
                .expect("relative path inside working directory should be allowed");
            assert!(output.contains("nested.txt"), "got: {output}");

            // no working directory configured -> restriction applies against cwd
            let err = run_search(None, true, Some(other.to_string_lossy().to_string()))
                .await
                .expect_err("no working directory means restriction against cwd");
            assert!(err.to_string().contains("outside the working directory"));

            // path inside the cwd -> allowed
            let output = run_search(None, true, Some("src".to_string()))
                .await
                .expect("path inside cwd should be allowed");
            assert!(
                !output.contains("files_searched: 0"),
                "search should have run over src, got: {output}"
            );

            std::fs::remove_dir_all(&dir).ok();
            std::fs::remove_dir_all(&other).ok();
        }
    }

    mod edge_cases {
        use super::*;

        #[test]
        fn regex_special_chars_escaped_in_literal() {
            // When mode is literal, special chars should be escaped
            // This is tested indirectly through pattern_str construction
            let pattern = "foo.bar";
            let escaped = regex::escape(pattern);
            let re = regex::Regex::new(&escaped).unwrap();

            // Should match literal "foo.bar", not "foo" + any char + "bar"
            assert!(re.is_match("foo.bar"));
            assert!(!re.is_match("fooxbar")); // Would match if regex unescaped
        }

        #[test]
        fn very_long_line_truncation_in_context() {
            // Context lines should also be truncated
            let long_line: String = "x".repeat(500);
            let truncated = truncate_line(&long_line);
            assert_eq!(truncated.chars().count(), 301); // 300 + '…'
        }
    }
}
