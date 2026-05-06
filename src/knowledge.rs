use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct Knowledge {
    pub name: String,
    pub sources: Vec<KnowledgeSource>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase", deny_unknown_fields)]
pub enum KnowledgeSource {
    /// An inline string.
    Text { value: String },

    /// A file path. Relative paths are resolved against the current working directory.
    File { path: std::path::PathBuf },
}

impl Knowledge {
    /// Resolve all knowledge sources into a formatted knowledge block.
    ///
    /// Returns an XML-style block with clear boundaries:
    /// ```xml
    /// <knowledge name="knowledge-name">
    /// [content from source 1]
    ///
    /// ---
    ///
    /// [content from source 2]
    /// </knowledge>
    /// ```
    ///
    /// Filters out sources that resolve to None.
    pub fn resolve(&self) -> Result<String, std::io::Error> {
        let resolved: Result<Vec<_>, _> =
            self.sources.iter().map(|source| source.resolve()).collect();
        let resolved = resolved?;
        let non_empty: Vec<_> = resolved.into_iter().flatten().collect();
        let content = non_empty.join("\n\n---\n\n");

        Ok(format!(
            "<knowledge name=\"{}\">\n{}\n</knowledge>",
            self.name, content
        ))
    }
}

impl KnowledgeSource {
    /// Resolve the source into its final string content.
    ///
    /// Returns `Ok(None)` if the source has no content (empty text or missing/empty file).
    /// Returns `Ok(Some(content))` if the source has content.
    pub fn resolve(&self) -> Result<Option<String>, std::io::Error> {
        match self {
            KnowledgeSource::Text { value } => {
                if value.is_empty() {
                    Ok(None)
                } else {
                    Ok(Some(value.clone()))
                }
            }
            KnowledgeSource::File { path } => {
                let resolved = if path.is_absolute() {
                    path.clone()
                } else {
                    std::env::current_dir()?.join(path)
                };
                if resolved.exists() {
                    let content = std::fs::read_to_string(&resolved)?;
                    if content.is_empty() {
                        Ok(None)
                    } else {
                        Ok(Some(content))
                    }
                } else {
                    Ok(None)
                }
            }
        }
    }
}
