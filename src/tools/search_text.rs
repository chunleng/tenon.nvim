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
    while pos < line.len() {
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
    if line.chars().count() > LINE_TRUNCATION_LIMIT {
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

            if let Some(ref matcher) = glob_matcher {
                let relative = entry
                    .path()
                    .strip_prefix(search_path)
                    .unwrap_or(entry.path());
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

            let path_str = entry.path().to_str().unwrap_or_default().to_string();
            files.push((path_str, text));
        }

        let result = perform_search(files, is_regex, &re, context_lines, max_files);

        Ok(serde_json::to_string(&result).unwrap_or_else(|_| {
            r#"{"files":[],"total_matches":0,"files_with_matches":0,"files_searched":0,"truncated_files":0}"#
                .to_string()
        }))
    }
}

fn perform_search(
    files: Vec<(String, String)>,
    is_regex: bool,
    re: &regex::Regex,
    context_lines: usize,
    max_files: Option<usize>,
) -> SearchResult {
    let mut file_results: Vec<FileEntry> = Vec::new();
    let mut total_matches: usize = 0;
    let files_searched = files.len();

    for (path, text) in files {
        let lines: Vec<&str> = text.lines().collect();
        let mut file_matches: Vec<MatchEntry> = Vec::new();

        for (line_idx, &line) in lines.iter().enumerate() {
            let span_matches = if is_regex {
                find_nonoverlapping_matches(re, line)
            } else {
                find_overlapping_matches(re, line)
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
        total_matches,
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
            let result = perform_search(files, false, &re, 0, None);

            assert_eq!(result.total_matches, 2);
            assert_eq!(result.files_with_matches, 2);
            assert_eq!(result.files[0].path, "file1.txt");
            assert_eq!(result.files[0].matches[0].line, "hello world");
        }

        #[test]
        fn regex_search() {
            let files = vec![("file1.txt".to_string(), "123 hello\n456 world".to_string())];
            let re = regex::Regex::new(r"\d+").unwrap();
            let result = perform_search(files, true, &re, 0, None);

            assert_eq!(result.total_matches, 2);
            assert_eq!(result.files[0].matches[0].column_end, 3);
        }

        #[test]
        fn context_lines() {
            let files = vec![(
                "file1.txt".to_string(),
                "line1\nline2\nline3\nline4\nline5".to_string(),
            )];
            let re = regex::Regex::new("line3").unwrap();
            let result = perform_search(files, false, &re, 1, None);

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
            let result = perform_search(files, false, &re, 0, Some(2));

            assert_eq!(result.files.len(), 2);
            assert_eq!(result.truncated_files, 1);
        }

        #[test]
        fn match_limit_clears_matches() {
            let files = vec![
                ("file1.txt".to_string(), "a\na\na".repeat(40).to_string()), // > 100 matches
            ];
            let re = regex::Regex::new("a").unwrap();
            let result = perform_search(files, false, &re, 0, None);

            assert!(result.total_matches > MATCH_LIMIT);
            assert!(result.files[0].matches.is_empty());
        }
    }

    mod next_char_boundary {
        use super::*;

        #[test]
        fn ascii_string() {
            assert_eq!(next_char_boundary("hello", 0), 1);
            assert_eq!(next_char_boundary("hello", 1), 2);
            assert_eq!(next_char_boundary("hello", 4), 5);
        }

        #[test]
        fn utf8_multibyte() {
            // '日' is 3 bytes, '本' is 3 bytes
            assert_eq!(next_char_boundary("日本語", 0), 3);
            assert_eq!(next_char_boundary("日本語", 3), 6);
        }

        #[test]
        fn at_end_of_string() {
            assert_eq!(next_char_boundary("a", 0), 1);
            assert_eq!(next_char_boundary("abc", 3), 3); // at the end, returns length
        }

        #[test]
        fn mixed_ascii_and_multibyte() {
            // 'a' is 1 byte, '日' is 3 bytes
            assert_eq!(next_char_boundary("a日", 0), 1);
            assert_eq!(next_char_boundary("a日", 1), 4);
        }

        #[test]
        fn emoji() {
            // '😀' is 4 bytes
            assert_eq!(next_char_boundary("😀😀", 0), 4);
            assert_eq!(next_char_boundary("😀😀", 4), 8);
        }
    }

    mod find_overlapping_matches {
        use super::*;

        #[test]
        fn no_matches() {
            let re = regex::Regex::new("foo").unwrap();
            let matches = find_overlapping_matches(&re, "bar baz");
            assert!(matches.is_empty());
        }

        #[test]
        fn single_match() {
            let re = regex::Regex::new("foo").unwrap();
            let matches = find_overlapping_matches(&re, "foo bar");
            assert_eq!(matches, vec![(0, 3)]);
        }

        #[test]
        fn multiple_non_overlapping_matches() {
            let re = regex::Regex::new("foo").unwrap();
            let matches = find_overlapping_matches(&re, "foo bar foo");
            assert_eq!(matches, vec![(0, 3), (8, 11)]);
        }

        #[test]
        fn overlapping_matches() {
            // Regex that matches "aa" - overlapping occurrences in "aaa"
            let re = regex::Regex::new("aa").unwrap();
            let matches = find_overlapping_matches(&re, "aaa");
            assert_eq!(matches, vec![(0, 2), (1, 3)]);
        }

        #[test]
        fn complex_overlapping() {
            // In "aaaa", "aa" appears at positions 0-2, 1-3, 2-4
            let re = regex::Regex::new("aa").unwrap();
            let matches = find_overlapping_matches(&re, "aaaa");
            assert_eq!(matches, vec![(0, 2), (1, 3), (2, 4)]);
        }

        #[test]
        fn empty_match_skipped() {
            // Regex that can match empty string should not infinite loop
            let re = regex::Regex::new("a*").unwrap();
            let matches = find_overlapping_matches(&re, "b");
            // Should not hang, and should handle empty matches gracefully
            // Empty matches at position 0 and 1 (after 'b')
            // The function should skip zero-length matches
            assert!(matches.iter().all(|(start, end)| start < end));
        }

        #[test]
        fn utf8_boundaries() {
            let re = regex::Regex::new("日").unwrap();
            let matches = find_overlapping_matches(&re, "日本語日");
            assert_eq!(matches, vec![(0, 3), (9, 12)]);
        }

        #[test]
        fn word_boundary() {
            let re = regex::Regex::new(r"\btest\b").unwrap();
            let matches = find_overlapping_matches(&re, "test testing test");
            // \btest\b matches "test" at start and end, not in "testing"
            assert_eq!(matches, vec![(0, 4), (13, 17)]);
        }
    }

    mod find_nonoverlapping_matches {
        use super::*;

        #[test]
        fn no_matches() {
            let re = regex::Regex::new("foo").unwrap();
            let matches = find_nonoverlapping_matches(&re, "bar baz");
            assert!(matches.is_empty());
        }

        #[test]
        fn multiple_non_overlapping() {
            let re = regex::Regex::new("foo").unwrap();
            let matches = find_nonoverlapping_matches(&re, "foo bar foo");
            assert_eq!(matches, vec![(0, 3), (8, 11)]);
        }

        #[test]
        fn overlapping_pattern_non_overlapping_result() {
            // "aa" appears overlapping in "aaa", but find_nonoverlapping should not overlap
            let re = regex::Regex::new("aa").unwrap();
            let matches = find_nonoverlapping_matches(&re, "aaa");
            // Standard regex finds only (0, 2), not (1, 3)
            assert_eq!(matches, vec![(0, 2)]);
        }

        #[test]
        fn regex_captures() {
            let re = regex::Regex::new(r"\d+").unwrap();
            let matches = find_nonoverlapping_matches(&re, "123 456 789");
            assert_eq!(matches, vec![(0, 3), (4, 7), (8, 11)]);
        }

        #[test]
        fn case_insensitive() {
            let re = regex::RegexBuilder::new("foo")
                .case_insensitive(true)
                .build()
                .unwrap();
            let matches = find_nonoverlapping_matches(&re, "FOO bar foo");
            assert_eq!(matches, vec![(0, 3), (8, 11)]);
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
                total_matches: 0,
                files_with_matches: 0,
                files_searched: 0,
                truncated_files: 0,
            };
            let json = serde_json::to_string(&result).unwrap();
            assert!(json.contains("\"files\":[]"));
            assert!(json.contains("\"total_matches\":0"));
        }

        #[test]
        fn result_with_matches() {
            let result = SearchResult {
                files: vec![FileEntry {
                    path: "test.rs".to_string(),
                    matches: vec![MatchEntry {
                        line_number: 1,
                        column_start: 0,
                        column_end: 4,
                        line: "test".to_string(),
                        context_before: vec![],
                        context_after: vec!["next".to_string()],
                    }],
                }],
                total_matches: 1,
                files_with_matches: 1,
                files_searched: 5,
                truncated_files: 0,
            };
            let json = serde_json::to_string(&result).unwrap();
            let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();

            assert_eq!(parsed["total_matches"], 1);
            assert_eq!(parsed["files_searched"], 5);
            assert_eq!(parsed["files"][0]["path"], "test.rs");
            assert_eq!(parsed["files"][0]["matches"][0]["line_number"], 1);
            assert_eq!(parsed["files"][0]["matches"][0]["column_start"], 0);
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
                column_start: 0,
                column_end: 3,
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

    mod edge_cases {
        use super::*;

        #[test]
        fn regex_special_chars_escaped_in_literal() {
            // When is_regex is false, special chars should be escaped
            // This is tested indirectly through pattern_str construction
            let pattern = "foo.bar";
            let escaped = regex::escape(pattern);
            let re = regex::Regex::new(&escaped).unwrap();

            // Should match literal "foo.bar", not "foo" + any char + "bar"
            assert!(re.is_match("foo.bar"));
            assert!(!re.is_match("fooxbar")); // Would match if regex unescaped
        }

        #[test]
        fn empty_pattern_matches_everything() {
            // Empty pattern in regex matches at every position
            let re = regex::Regex::new("").unwrap();
            let matches = find_nonoverlapping_matches(&re, "test");
            // Empty string matches between each character
            assert!(!matches.is_empty());
        }

        #[test]
        fn very_long_line_truncation_in_context() {
            // Context lines should also be truncated
            let long_line: String = "x".repeat(500);
            let truncated = truncate_line(&long_line);
            assert_eq!(truncated.chars().count(), 301); // 300 + '…'
        }

        #[test]
        fn utf8_character_boundaries_in_overlap() {
            // Ensure we don't split multibyte characters when finding overlapping matches
            let re = regex::Regex::new("日本").unwrap();
            let text = "日本日本";
            let matches = find_overlapping_matches(&re, text);
            // "日本" at 0-6 and 6-12 (no overlap because they're adjacent)
            assert_eq!(matches, vec![(0, 6), (6, 12)]);
        }

        #[test]
        fn multiple_matches_on_same_line() {
            let re = regex::Regex::new("a").unwrap();
            let matches = find_overlapping_matches(&re, "aaa");
            // 'a' matches at positions 0, 1, 2 - all non-overlapping since 'a' is 1 byte
            assert_eq!(matches, vec![(0, 1), (1, 2), (2, 3)]);
        }
    }
}
