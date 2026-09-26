mod rust;

use crate::tools::{ToolCore, ToolCoreCall};
use rig::tool::{ToolContext, ToolExecutionError};
use serde::Deserialize;
use serde_json::json;
use std::path::PathBuf;

#[derive(Deserialize, PartialEq, Debug)]
#[serde(rename_all = "lowercase")]
pub enum SupportedLanguages {
    Rust,
}

use crate::agent::worker::full::{GoalOrientedWorker, GoalResult};
use crate::get_application_config;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SearchDependencyCodeArgs {
    pub prompt: String,
    pub language: SupportedLanguages,
    pub dependency: String,
    pub version: Option<String>,
}

pub struct SearchDependencyCode;

impl ToolCore for SearchDependencyCode {
    const NAME: &'static str = "search_dependency_code";
    type Error = ToolExecutionError;
    type Args = SearchDependencyCodeArgs;
    type Output = String;
    type Call = SearchDependencyCodeCall;

    fn description(&self) -> String {
        "Search a project dependency's source code to understand how it works \
         or find implementation details."
            .to_string()
    }

    fn parameters(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "prompt": {
                    "type": "string",
                    "description": "What to investigate in the dependency's code"
                },
                "language": {
                    "type": "string",
                    "enum": ["rust"],
                    "description": "Programming language"
                },
                "dependency": {
                    "type": "string",
                    "description": "Dependency name as it appears in the project's manifest"
                },
                "version": {
                    "type": "string",
                    "description": "Exact dependency version. Autoresolve when omitted. Required when multiple versions exist"
                }
            },
            "required": ["prompt", "language", "dependency"]
        })
    }

    async fn init_call(
        &self,
        _context: &mut ToolContext,
        args: Self::Args,
    ) -> Result<Self::Call, Self::Error> {
        Ok(SearchDependencyCodeCall { args })
    }
}

pub struct SearchDependencyCodeCall {
    args: SearchDependencyCodeArgs,
}

impl ToolCoreCall for SearchDependencyCodeCall {
    type Error = ToolExecutionError;
    type Output = String;

    async fn result(self, _context: &mut ToolContext) -> Result<Self::Output, Self::Error> {
        let args = self.args;

        // Validate project type, dependency existence, and version match.
        // Resolves exact version from Cargo.lock when not provided.
        let resolved_version = validate_dependency_source(&args)?;

        // cargo metadata resolves and downloads the dependency source path.
        let source_path = resolve_source_path(&args, &resolved_version)?;

        let config = get_application_config();
        let agent_config = config
            .agents
            .get(&config.default_agent)
            .ok_or_else(|| ToolExecutionError::other("No default agent configured"))?;
        let model = agent_config.model.clone();

        let tool_names = vec![
            "search_text".to_string(),
            "read_file".to_string(),
            "list_files".to_string(),
        ];

        let mut agent = GoalOrientedWorker::new(model, vec![], tool_names);

        let task = format!(
            "Investigate the following in the dependency source code located at:\n{}\n\n\
             {}\n\n\
             All paths must be within the dependency source directory above.",
            source_path.display(),
            args.prompt
        );

        match agent.perform_task(&task).await {
            GoalResult::Answer(answer) => Ok(answer),
            GoalResult::NoAnswer(Some(explanation)) => Err(ToolExecutionError::other(format!(
                "Agent could not complete the task: {}",
                explanation
            ))),
            GoalResult::NoAnswer(None) => Err(ToolExecutionError::other(
                "Agent timed out without producing an answer",
            )),
        }
    }
}

fn validate_dependency_source(
    args: &SearchDependencyCodeArgs,
) -> Result<String, ToolExecutionError> {
    match args.language {
        SupportedLanguages::Rust => {
            rust::validate_rust_dependency(&args.dependency, args.version.as_deref())
        }
    }
}

fn resolve_source_path(
    args: &SearchDependencyCodeArgs,
    resolved_version: &str,
) -> Result<PathBuf, ToolExecutionError> {
    match args.language {
        SupportedLanguages::Rust => {
            rust::resolve_rust_source_path(&args.dependency, resolved_version)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn supported_languages_deserializes_rust() {
        let json = r#"{"prompt":"test","language":"rust","dependency":"toml"}"#;
        let args: SearchDependencyCodeArgs = serde_json::from_str(json).unwrap();
        assert_eq!(args.language, SupportedLanguages::Rust);
        assert!(
            args.version.is_none(),
            "Version should be None when not provided"
        );
    }

    #[test]
    fn supported_languages_rejects_unsupported() {
        let json =
            r#"{"prompt":"test","language":"python","dependency":"toml","version":"0.8.23"}"#;
        let result: Result<SearchDependencyCodeArgs, _> = serde_json::from_str(json);
        assert!(result.is_err());
    }
}
