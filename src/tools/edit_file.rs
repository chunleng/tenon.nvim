use crate::utils::GLOBAL_EXECUTION_HANDLER;
use crate::utils::path_from_str;
use regex::RegexBuilder;

use rig::tool::{Tool, ToolContext, ToolExecutionError};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::fs;

#[derive(Deserialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ReplaceMode {
    One,
    All,
}

#[derive(Deserialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum SearchMode {
    Literal,
    Regex,
}

/// Returns the 1-based line number at the given byte offset within `content`.
fn line_at(content: &str, byte_offset: usize) -> usize {
    content[..byte_offset].lines().count() + 1
}

/// Core edit logic: performs find/replace on content.
/// Returns (new_content, edits_info) where edits_info contains line numbers.
/// Ensures trailing newline when editing at EOF.
fn perform_edit(
    content: &str,
    search: &str,
    replace: &str,
    replace_mode: &ReplaceMode,
    search_mode: &SearchMode,
) -> Result<(String, Vec<serde_json::Value>), std::io::Error> {
    // Check if edit touches EOF (need to ensure trailing newline).
    // No-op matches (matched text == replacement) are excluded.
    let edits_at_end = if *search_mode == SearchMode::Regex {
        RegexBuilder::new(search)
            .dot_matches_new_line(true)
            .build()
            .map(|re| {
                re.find_iter(content)
                    .any(|m| m.end() == content.len() && m.as_str() != replace)
            })
            .unwrap_or(false)
    } else {
        search != replace && content.ends_with(search)
    };

    let (new_content, edits) = if *search_mode == SearchMode::Regex {
        let re = RegexBuilder::new(search)
            .dot_matches_new_line(true)
            .build()
            .map_err(|e| {
                std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    format!("Bad regex: {}", e),
                )
            })?;

        let raw_matches: Vec<_> = re.find_iter(content).collect();
        let matches: Vec<_> = raw_matches
            .iter()
            .filter(|m| m.as_str() != replace)
            .cloned()
            .collect();
        let match_count = matches.len();

        if match_count == 0 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                if !raw_matches.is_empty() {
                    "Matched text equals replacement".to_string()
                } else {
                    "No match found".to_string()
                },
            ));
        }

        if *replace_mode == ReplaceMode::One && match_count > 1 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!("{} matches. Use 'all' or narrow search", match_count),
            ));
        }

        if *replace_mode == ReplaceMode::All && match_count > 20 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!(
                    "{} matches exceed the 20-replacement limit for 'all' mode",
                    match_count
                ),
            ));
        }

        let edits: Vec<serde_json::Value> = matches
            .iter()
            .map(|m| {
                json!({
                    "line": line_at(content, m.start()),
                    "text_replaced": m.as_str(),
                })
            })
            .collect();

        let replacements: Vec<_> = match replace_mode {
            ReplaceMode::One => matches.iter().take(1).collect(),
            _ => matches.iter().collect(),
        };
        let mut result = String::new();
        let mut last_end = 0;
        for m in replacements {
            result.push_str(&content[last_end..m.start()]);
            result.push_str(replace);
            last_end = m.end();
        }
        result.push_str(&content[last_end..]);

        (result, edits)
    } else {
        let raw_matches: Vec<_> = content.match_indices(search).collect();
        let matches: Vec<_> = raw_matches
            .iter()
            .filter(|(_, m)| *m != replace)
            .cloned()
            .collect();
        let match_count = matches.len();

        if match_count == 0 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                if !raw_matches.is_empty() {
                    "Matched text equals replacement".to_string()
                } else {
                    "No match found".to_string()
                },
            ));
        }

        if *replace_mode == ReplaceMode::One && match_count > 1 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!("{} matches. Use 'all' or narrow search", match_count),
            ));
        }

        if *replace_mode == ReplaceMode::All && match_count > 20 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!(
                    "{} matches exceed the 20-replacement limit for 'all' mode",
                    match_count
                ),
            ));
        }

        let edits: Vec<serde_json::Value> = matches
            .iter()
            .map(|(offset, _)| {
                json!({
                    "line": line_at(content, *offset),
                })
            })
            .collect();

        let new_content = match replace_mode {
            ReplaceMode::One => content.replacen(search, replace, 1),
            _ => content.replace(search, replace),
        };

        (new_content, edits)
    };

    // Ensure trailing newline when editing at EOF
    let final_content = if edits_at_end && !new_content.ends_with('\n') {
        format!("{}\n", new_content)
    } else {
        new_content
    };

    Ok((final_content, edits))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EditFileArgs {
    pub filepath: String,
    pub search: String,
    pub replace: String,
    pub replace_mode: Option<ReplaceMode>,
    pub search_mode: SearchMode,
}

#[derive(Deserialize, Serialize, Clone)]
pub struct EditFile;

impl Tool for EditFile {
    const NAME: &'static str = "edit_file";
    type Error = ToolExecutionError;
    type Args = EditFileArgs;
    type Output = String;

    fn description(&self) -> String {
        "Find → replace. Non-existent file → auto-created (use search='', replace='content')"
            .to_string()
    }

    fn parameters(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "filepath": { "type": "string", "description": "File path" },
                "search": { "type": "string", "description": "Search text or regex (see search_mode)" },
                "replace": { "type": "string", "description": "Replacement text" },
                "replace_mode": {
                    "type": "string",
                    "enum": ["one", "all"],
                    "description": "one = first match (error if >1). all = every match"
                },
                "search_mode": {
                    "type": "string",
                    "enum": ["literal", "regex"],
                    "description": "literal = exact match. regex = pattern, dot matches \\n"
                }
            },
            "required": ["filepath", "search", "replace", "search_mode"]
        })
    }

    async fn call(
        &self,
        _context: &mut ToolContext,
        args: Self::Args,
    ) -> Result<Self::Output, Self::Error> {
        let replace_mode = args.replace_mode.unwrap_or(ReplaceMode::One);
        let path = path_from_str(&args.filepath);

        let content = match fs::read_to_string(&path) {
            Ok(c) => c,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                if let Some(parent) = path.parent()
                    && !parent.as_os_str().is_empty()
                    && let Err(e) = fs::create_dir_all(parent)
                {
                    return Err(ToolExecutionError::other(format!(
                        "mkdir fail '{}': {}",
                        args.filepath, e
                    )));
                }
                String::new()
            }
            Err(e) => {
                return Err(ToolExecutionError::other(format!(
                    "Read fail '{}': {}",
                    args.filepath, e
                )));
            }
        };

        let (final_content, edits) = perform_edit(
            &content,
            &args.search,
            &args.replace,
            &replace_mode,
            &args.search_mode,
        )
        .map_err(|e| ToolExecutionError::other(e.to_string()))?;

        fs::write(path, &final_content).map_err(|e| {
            ToolExecutionError::other(format!("Write fail '{}': {}", args.filepath, e))
        })?;

        let _ = GLOBAL_EXECUTION_HANDLER
            .execute_rust_on_main_thread(|| Ok(nvim_oxi::api::command("checktime")?));

        serde_yaml::to_string(&json!({
            "successful_edits": edits,
            "count": edits.len(),
        }))
        .map_err(|e| ToolExecutionError::other(format!("Serialize failed: {}", e)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_edit_at_eof_adds_trailing_newline() {
        let content = "hello world";
        let (result, _) = perform_edit(
            content,
            "world",
            "universe",
            &ReplaceMode::One,
            &SearchMode::Literal,
        )
        .unwrap();
        assert_eq!(result, "hello universe\n");
    }

    #[test]
    fn test_edit_at_eof_preserves_existing_newline() {
        let content = "hello world\n";
        let (result, _) = perform_edit(
            content,
            "world",
            "universe",
            &ReplaceMode::One,
            &SearchMode::Literal,
        )
        .unwrap();
        assert_eq!(result, "hello universe\n");
    }

    #[test]
    fn test_edit_in_middle_does_not_add_newline() {
        let content = "hello world foo";
        let (result, _) = perform_edit(
            content,
            "hello",
            "hey",
            &ReplaceMode::One,
            &SearchMode::Literal,
        )
        .unwrap();
        assert_eq!(result, "hey world foo");
    }

    #[test]
    fn test_edit_in_middle_preserves_existing_newline() {
        let content = "hello world\n";
        let (result, _) = perform_edit(
            content,
            "hello",
            "hey",
            &ReplaceMode::One,
            &SearchMode::Literal,
        )
        .unwrap();
        assert_eq!(result, "hey world\n");
    }

    #[test]
    fn test_regex_edit_at_eof_adds_newline() {
        let content = "hello world";
        let (result, _) = perform_edit(
            content,
            "w.*d",
            "universe",
            &ReplaceMode::One,
            &SearchMode::Regex,
        )
        .unwrap();
        assert_eq!(result, "hello universe\n");
    }

    #[test]
    fn test_replace_all_at_eof_adds_newline() {
        let content = "foo bar foo";
        let (result, _) = perform_edit(
            content,
            "foo",
            "baz",
            &ReplaceMode::All,
            &SearchMode::Literal,
        )
        .unwrap();
        assert_eq!(result, "baz bar baz\n");
    }

    #[test]
    fn test_replace_all_in_middle_no_newline() {
        let content = "foo bar foo baz";
        let (result, _) = perform_edit(
            content,
            "foo",
            "baz",
            &ReplaceMode::All,
            &SearchMode::Literal,
        )
        .unwrap();
        assert_eq!(result, "baz bar baz baz");
    }

    #[test]
    fn test_edit_eof_multiline() {
        let content = "line1\nline2\nlast";
        let (result, _) = perform_edit(
            content,
            "last",
            "final",
            &ReplaceMode::One,
            &SearchMode::Literal,
        )
        .unwrap();
        assert_eq!(result, "line1\nline2\nfinal\n");
    }

    #[test]
    fn test_no_match_returns_error() {
        let content = "hello world";
        let result = perform_edit(
            content,
            "notfound",
            "replacement",
            &ReplaceMode::One,
            &SearchMode::Literal,
        );
        assert!(result.is_err());
    }

    #[test]
    fn test_multiple_matches_one_mode_error() {
        let content = "foo foo foo";
        let result = perform_edit(
            content,
            "foo",
            "bar",
            &ReplaceMode::One,
            &SearchMode::Literal,
        );
        assert!(result.is_err());
    }

    #[test]
    fn test_replace_all_multiple_matches() {
        let content = "foo bar foo bar";
        let (result, _) = perform_edit(
            content,
            "foo",
            "baz",
            &ReplaceMode::All,
            &SearchMode::Literal,
        )
        .unwrap();
        assert_eq!(result, "baz bar baz bar");
    }

    #[test]
    fn test_empty_content_empty_search_creates_content() {
        let (result, _) = perform_edit(
            "",
            "",
            "hello world",
            &ReplaceMode::One,
            &SearchMode::Literal,
        )
        .unwrap();
        assert_eq!(result, "hello world\n");
    }

    #[test]
    fn test_empty_content_nonempty_search_no_match() {
        let result = perform_edit("", "foo", "bar", &ReplaceMode::One, &SearchMode::Literal);
        assert!(result.is_err());
    }

    #[test]
    fn test_replace_all_exceeds_limit_returns_error() {
        let content = "foo ".repeat(21);
        let result = perform_edit(
            &content,
            "foo",
            "bar",
            &ReplaceMode::All,
            &SearchMode::Literal,
        );
        assert!(result.is_err());
    }

    #[test]
    fn test_literal_noop_not_counted() {
        let content = "hello world";
        let result = perform_edit(
            content,
            "hello",
            "hello",
            &ReplaceMode::One,
            &SearchMode::Literal,
        );
        assert!(result.is_err());
    }

    #[test]
    fn test_regex_noop_filtered_one_mode_succeeds() {
        let content = "foo bar foo";
        let (result, edits) = perform_edit(
            content,
            "foo|bar",
            "foo",
            &ReplaceMode::One,
            &SearchMode::Regex,
        )
        .unwrap();
        assert_eq!(edits.len(), 1);
        assert_eq!(result, "foo foo foo");
    }

    #[test]
    fn test_regex_all_noop_returns_error() {
        let content = "abc abc";
        let result = perform_edit(content, "abc", "abc", &ReplaceMode::All, &SearchMode::Regex);
        assert!(result.is_err());
    }

    #[test]
    fn test_replace_all_at_limit_succeeds() {
        let content = "foo ".repeat(20);
        let (result, edits) = perform_edit(
            &content,
            "foo",
            "bar",
            &ReplaceMode::All,
            &SearchMode::Literal,
        )
        .unwrap();
        assert_eq!(edits.len(), 20);
        assert_eq!(result, "bar ".repeat(20));
    }
}
