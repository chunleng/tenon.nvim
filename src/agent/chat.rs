use std::sync::{Arc, RwLock};

use crate::{
    chat::workflow::Workflow,
    chat::{ActiveWorkflow, LogWindow},
    clients::{ChatAgent, SupportedModels, get_agent},
    directive::{Directive, DirectiveSource, directive_path},
    tools::resolve_tools,
};

#[derive(Debug, Clone)]
pub struct TenonAgent {
    pub model: SupportedModels,
    pub directive: Vec<Directive>,
    pub tool_names: Vec<String>,
    pub workflows: Vec<Arc<Workflow>>,
}

impl TenonAgent {
    pub fn new(
        model: SupportedModels,
        directive: Vec<Directive>,
        tools: &[impl AsRef<str>],
        workflows: Vec<Arc<Workflow>>,
    ) -> Self {
        Self {
            model,
            directive,
            tool_names: tools.iter().map(|t| t.as_ref().to_string()).collect(),
            workflows,
        }
    }

    pub fn build_chat_adapter(
        &self,
        workflow_context: Arc<RwLock<Option<ActiveWorkflow>>>,
        log_window: Arc<RwLock<LogWindow>>,
    ) -> ChatAgent {
        let mut combined = vec![Directive {
            condition: None,
            source: DirectiveSource::File {
                paths: vec![directive_path("tenon_constitution.md")],
            },
        }];
        combined.extend(self.directive.iter().cloned());

        let mut tools = resolve_tools(&self.tool_names);

        let has_active = workflow_context
            .read()
            .map(|g| g.is_some())
            .unwrap_or(false);

        if has_active {
            use crate::tools::end_workflow::EndWorkflow;
            use crate::tools::navigate_workflow::NavigateWorkflow;
            tools.push(Box::new(NavigateWorkflow {
                active_workflow: workflow_context.clone(),
                log_window: log_window.clone(),
            }));
            tools.push(Box::new(EndWorkflow {
                active_workflow: workflow_context,
                log_window,
            }));
        } else if !self.workflows.is_empty() {
            use crate::tools::start_workflow::StartWorkflow;
            tools.push(Box::new(StartWorkflow {
                workflows: self.workflows.clone(),
                active_workflow: workflow_context.clone(),
                log_window: log_window.clone(),
            }));
        }

        get_agent(self.model.clone(), combined, tools, true)
    }
}
