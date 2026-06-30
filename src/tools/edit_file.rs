use crate::utils::GLOBAL_EXECUTION_HANDLER;
use regex::RegexBuilder;
use rig::completion::ToolDefinition;
use rig::tool::{Tool, ToolError};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::fs;
use std::path::Path;

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
    replace_mode: &str,
    search_mode: &str,
) -> Result<(String, Vec<serde_json::Value>), std::io::Error> {
    // Check if edit touches EOF (need to ensure trailing newline)
    let edits_at_end = if search_mode == "regex" {
        RegexBuilder::new(search)
            .dot_matches_new_line(true)
            .build()
            .map(|re| re.find_iter(content).any(|m| m.end() == content.len()))
            .unwrap_or(false)
    } else {
        content.ends_with(search)
    };

    let (new_content, edits) = if search_mode == "regex" {
        let re = RegexBuilder::new(search)
            .dot_matches_new_line(true)
            .build()
            .map_err(|e| {
                std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    format!("Bad regex: {}", e),
                )
            })?;

        let matches: Vec<_> = re.find_iter(content).collect();
        let match_count = matches.len();

        if match_count == 0 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "No match found".to_string(),
            ));
        }

        if replace_mode == "one" && match_count > 1 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!("{} matches. Use 'all' or narrow search", match_count),
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

        let result = match replace_mode {
            "one" => re.replace(content, regex::NoExpand(replace)).to_string(),
            _ => re
                .replace_all(content, regex::NoExpand(replace))
                .to_string(),
        };

        (result, edits)
    } else {
        let matches: Vec<_> = content.match_indices(search).collect();
        let match_count = matches.len();

        if match_count == 0 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "No match found".to_string(),
            ));
        }

        if replace_mode == "one" && match_count > 1 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!("{} matches. Use 'all' or narrow search", match_count),
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
            "one" => content.replacen(search, replace, 1),
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
    pub replace_mode: Option<String>,
    pub search_mode: Option<String>,
}

#[derive(Deserialize, Serialize, Clone)]
pub struct EditFile;

impl Tool for EditFile {
    const NAME: &'static str = "edit_file";
    type Error = ToolError;
    type Args = EditFileArgs;
    type Output = String;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: "edit_file".to_string(),
            description: "Find → replace. 'one' errors if >1 match. Example: empty file → search='', replace='new content'".to_string(),
            parameters: json!({
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
                        "description": "literal = exact match (default). regex = pattern, dot matches \\n"
                    }
                },
                "required": ["filepath", "search", "replace"]
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        let replace_mode = args.replace_mode.unwrap_or_else(|| "one".to_string());
        let search_mode = args.search_mode.unwrap_or_else(|| "literal".to_string());
        let path = Path::new(&args.filepath);

        if !["one", "all"].contains(&replace_mode.as_str()) {
            return Err(ToolError::ToolCallError(Box::new(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!("Bad replace_mode '{}'. Use 'one' or 'all'", replace_mode),
            ))));
        }

        if !["literal", "regex"].contains(&search_mode.as_str()) {
            return Err(ToolError::ToolCallError(Box::new(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!(
                    "Bad search_mode '{}'. Use 'literal' or 'regex'",
                    search_mode
                ),
            ))));
        }

        let content = fs::read_to_string(path).map_err(|e| {
            ToolError::ToolCallError(Box::new(std::io::Error::new(
                e.kind(),
                format!("Read fail '{}': {}", args.filepath, e),
            )))
        })?;

        let (final_content, edits) = perform_edit(
            &content,
            &args.search,
            &args.replace,
            &replace_mode,
            &search_mode,
        )
        .map_err(|e| ToolError::ToolCallError(Box::new(e)))?;

        fs::write(path, &final_content).map_err(|e| {
            ToolError::ToolCallError(Box::new(std::io::Error::new(
                e.kind(),
                format!("Write fail '{}': {}", args.filepath, e),
            )))
        })?;

        let _ = GLOBAL_EXECUTION_HANDLER.execute_on_main_thread("vim.cmd('checktime')");

        Ok(json!({
            "successful_edits": edits,
            "count": edits.len(),
        })
        .to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_edit_at_eof_adds_trailing_newline() {
        let content = "hello world";
        let (result, _) = perform_edit(content, "world", "universe", "one", "literal").unwrap();
        assert_eq!(result, "hello universe\n");
    }

    #[test]
    fn test_edit_at_eof_preserves_existing_newline() {
        let content = "hello world\n";
        let (result, _) = perform_edit(content, "world", "universe", "one", "literal").unwrap();
        assert_eq!(result, "hello universe\n");
    }

    #[test]
    fn test_edit_in_middle_does_not_add_newline() {
        let content = "hello world foo";
        let (result, _) = perform_edit(content, "hello", "hey", "one", "literal").unwrap();
        assert_eq!(result, "hey world foo");
    }

    #[test]
    fn test_edit_in_middle_preserves_existing_newline() {
        let content = "hello world\n";
        let (result, _) = perform_edit(content, "hello", "hey", "one", "literal").unwrap();
        assert_eq!(result, "hey world\n");
    }

    #[test]
    fn test_regex_edit_at_eof_adds_newline() {
        let content = "hello world";
        let (result, _) = perform_edit(content, "w.*d", "universe", "one", "regex").unwrap();
        assert_eq!(result, "hello universe\n");
    }

    #[test]
    fn test_replace_all_at_eof_adds_newline() {
        let content = "foo bar foo";
        let (result, _) = perform_edit(content, "foo", "baz", "all", "literal").unwrap();
        assert_eq!(result, "baz bar baz\n");
    }

    #[test]
    fn test_replace_all_in_middle_no_newline() {
        let content = "foo bar foo baz";
        let (result, _) = perform_edit(content, "foo", "baz", "all", "literal").unwrap();
        assert_eq!(result, "baz bar baz baz");
    }

    #[test]
    fn test_edit_eof_multiline() {
        let content = "line1\nline2\nlast";
        let (result, _) = perform_edit(content, "last", "final", "one", "literal").unwrap();
        assert_eq!(result, "line1\nline2\nfinal\n");
    }

    #[test]
    fn test_no_match_returns_error() {
        let content = "hello world";
        let result = perform_edit(content, "notfound", "replacement", "one", "literal");
        assert!(result.is_err());
    }

    #[test]
    fn test_multiple_matches_one_mode_error() {
        let content = "foo foo foo";
        let result = perform_edit(content, "foo", "bar", "one", "literal");
        assert!(result.is_err());
    }

    #[test]
    fn test_replace_all_multiple_matches() {
        let content = "foo bar foo bar";
        let (result, _) = perform_edit(content, "foo", "baz", "all", "literal").unwrap();
        assert_eq!(result, "baz bar baz bar");
    }
}
