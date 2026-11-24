use std::fs;
use std::ops::Range;
use std::path::Path;

use anyhow::{Context, Result};
use pulldown_cmark::{Event, MetadataBlockKind, Options, Parser, Tag};
use serde_json::Value;

#[derive(Debug)]
pub struct TenonMarkdown {
    pub frontmatter: Value,

    // TODO: remove unused after applying loading of Markdown using this utils for directive and
    // choreo move instructions
    #[warn(unused)]
    pub body: String,
}

impl TenonMarkdown {
    pub fn parse_file(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        let source = fs::read_to_string(path)
            .with_context(|| format!("failed to read Markdown file {}", path.display()))?;

        let (frontmatter, body) = parse_frontmatter(&source)?;
        let comment_ranges = find_comment_ranges(body);
        Ok(Self {
            frontmatter,
            body: remove_comments(body, &comment_ranges),
        })
    }
}

fn parse_frontmatter(source: &str) -> Result<(Value, &str)> {
    let metadata = Parser::new_ext(source, Options::ENABLE_YAML_STYLE_METADATA_BLOCKS)
        .into_offset_iter()
        .find_map(|(event, range)| match event {
            Event::Start(Tag::MetadataBlock(MetadataBlockKind::YamlStyle)) if range.start == 0 => {
                Some(range)
            }
            _ => None,
        });
    let (frontmatter, body) = if let Some(range) = metadata {
        let block = source[range.clone()].trim_end_matches(['\r', '\n']);
        let start = block.find('\n').unwrap() + 1;
        let end = block.rfind('\n').unwrap() + 1;
        let frontmatter =
            serde_yaml::from_str(&block[start..end]).context("invalid YAML frontmatter")?;
        let body = &source[range.end..];
        let body = body
            .strip_prefix("\r\n")
            .or_else(|| body.strip_prefix('\n'))
            .unwrap_or(body);
        (frontmatter, body)
    } else {
        (Value::Null, source)
    };
    Ok((frontmatter, body))
}

fn find_comment_ranges(body: &str) -> Vec<Range<usize>> {
    let mut ranges = Vec::new();
    for (event, range) in Parser::new(body).into_offset_iter() {
        match event {
            Event::Start(Tag::HtmlBlock) => {
                find_html_block_comments(&body[range.clone()], range.start, &mut ranges);
            }
            Event::InlineHtml(_) if body[range.clone()].starts_with("<!--") => {
                ranges.push(range);
            }
            _ => {}
        }
    }
    ranges
}

fn find_html_block_comments(html: &str, block_start: usize, ranges: &mut Vec<Range<usize>>) {
    let mut pos = 0;
    let mut in_tag = false;
    let mut quote = None;
    let mut raw_tag = None;
    while pos < html.len() {
        let rest = &html[pos..];
        if !in_tag && rest.starts_with("<!--") {
            let end = rest.find("-->").map_or(html.len(), |end| pos + end + 3);
            ranges.push(block_start + pos..block_start + end);
            pos = end;
        } else {
            let ch = rest.chars().next().unwrap();
            if in_tag {
                match (quote, ch) {
                    (Some(q), c) if q == c => quote = None,
                    (None, '\'' | '"') => quote = Some(ch),
                    (None, '>') => {
                        in_tag = false;
                        if let Some(tag) = raw_tag.take() {
                            let closing = format!("</{tag}");
                            let remaining = &html.as_bytes()[pos + 1..];
                            let offset = remaining.windows(closing.len()).enumerate().find_map(
                                |(index, window)| {
                                    (window.eq_ignore_ascii_case(closing.as_bytes())
                                        && remaining.get(index + closing.len()).is_none_or(|c| {
                                            c.is_ascii_whitespace() || *c == b'>' || *c == b'/'
                                        }))
                                    .then_some(index)
                                },
                            );
                            pos = offset.map_or(html.len(), |offset| pos + 1 + offset);
                            continue;
                        }
                    }
                    _ => {}
                }
            } else if ch == '<' {
                in_tag = true;
                for tag in ["script", "style"] {
                    let bytes = rest.as_bytes();
                    let end = tag.len() + 1;
                    if bytes
                        .get(1..end)
                        .is_some_and(|name| name.eq_ignore_ascii_case(tag.as_bytes()))
                        && bytes
                            .get(end)
                            .is_some_and(|c| c.is_ascii_whitespace() || *c == b'>' || *c == b'/')
                    {
                        raw_tag = Some(tag);
                        break;
                    }
                }
            }
            pos += ch.len_utf8();
        }
    }
}

fn remove_comments(body: &str, ranges: &[Range<usize>]) -> String {
    let mut stripped = String::with_capacity(body.len());
    let mut comment = 0;
    let mut line_start = 0;
    for line in body.split_inclusive('\n') {
        let line_end = line_start + line.len();
        let mut cursor = line_start;
        let mut without_comments = String::new();
        let mut has_comment = false;
        while comment < ranges.len() && ranges[comment].start < line_end {
            let range = &ranges[comment];
            if range.end <= line_start {
                comment += 1;
                continue;
            }
            has_comment = true;
            without_comments.push_str(&body[cursor..range.start.max(cursor)]);
            cursor = range.end.min(line_end);
            if range.end <= line_end {
                comment += 1;
            } else {
                break;
            }
        }
        without_comments.push_str(&body[cursor..line_end]);
        if !has_comment || !without_comments.trim().is_empty() {
            stripped.push_str(&without_comments);
        }
        line_start = line_end;
    }
    stripped
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;

    use serde_json::{Value, json};

    use super::TenonMarkdown;

    struct Workspace(PathBuf);

    impl Workspace {
        fn new() -> Self {
            let path =
                std::env::temp_dir().join(format!("tenon-markdown-{}", uuid::Uuid::new_v4()));
            fs::create_dir(&path).unwrap();
            Self(path)
        }

        fn write(&self, name: &str, contents: impl AsRef<[u8]>) -> PathBuf {
            let path = self.0.join(name);
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
    fn parses_yaml_frontmatter_and_strips_html_comments_without_reformatting() {
        let work = Workspace::new();
        let file = work.write(
            "document.md",
            concat!(
                "---\r\n",
                "title: 'Example'\r\n",
                "enabled: true\r\n",
                "tags: [one, two]\r\n",
                "nested:\r\n  count: 3\r\n",
                "---\r\n",
                "# Heading\r\n\r\n",
                "before <!-- one --> middle <!-- two\r\nlines --> after\r\n\r\n",
                "<div>\r\n<!-- hidden -->\r\nVisible <em>HTML</em>\r\n</div>\r\n\r\n",
                "`<!-- inline code -->`\r\n\r\n",
                "```html\r\n<!-- fenced code -->\r\n```\r\n\r\n",
                "    <!-- indented code -->\r\n\r\n",
                "<script>\r\nconst text = '<!-- script code -->';\r\n</script>\r\n",
            ),
        );

        let document = TenonMarkdown::parse_file(&file).unwrap();

        assert_eq!(
            document.frontmatter,
            json!({"title": "Example", "enabled": true, "tags": ["one", "two"], "nested": {"count": 3}})
        );
        assert_eq!(
            document.body,
            concat!(
                "# Heading\r\n\r\n",
                "before  middle  after\r\n\r\n",
                "<div>\r\nVisible <em>HTML</em>\r\n</div>\r\n\r\n",
                "`<!-- inline code -->`\r\n\r\n",
                "```html\r\n<!-- fenced code -->\r\n```\r\n\r\n",
                "    <!-- indented code -->\r\n\r\n",
                "<script>\r\nconst text = '<!-- script code -->';\r\n</script>\r\n",
            )
        );
    }

    #[test]
    fn without_frontmatter_uses_null_and_preserves_markdown() {
        let work = Workspace::new();
        let file = work.write("plain.md", "# Title\n\nText <!-- hidden --> **bold**.\n");

        let document = TenonMarkdown::parse_file(&file).unwrap();

        assert_eq!(document.frontmatter, Value::Null);
        assert_eq!(document.body, "# Title\n\nText  **bold**.\n");
    }

    #[test]
    fn removes_comment_only_lines_and_preserves_other_lines() {
        let work = Workspace::new();
        let file = work.write(
            "comments.md",
            concat!(
                "First\n",
                "<!-- hello! -->\n",
                "  <!-- hello! -->  \n",
                "<!-- one --> <!-- two -->\n",
                "  <!-- multiline\n",
                "  comment -->  \n",
                "\n",
                "before <!-- inline --> after\n\n",
                "    <!-- indented code -->\n\n",
                "  <!-- last -->  ",
            ),
        );

        let document = TenonMarkdown::parse_file(&file).unwrap();

        assert_eq!(
            document.body,
            "First\n\nbefore  after\n\n    <!-- indented code -->\n\n"
        );
    }

    #[test]
    fn rejects_invalid_yaml_missing_files_and_non_utf8() {
        let work = Workspace::new();
        let invalid_yaml = work.write("invalid.md", "---\ntitle: [broken\n---\nBody\n");
        let invalid_utf8 = work.write("non-utf8.md", b"---\ntitle: valid\n---\nbody\xff");
        let missing = work.0.join("missing.md");

        assert!(TenonMarkdown::parse_file(&invalid_yaml).is_err());
        assert!(TenonMarkdown::parse_file(&invalid_utf8).is_err());
        assert!(TenonMarkdown::parse_file(&missing).is_err());
    }
}
