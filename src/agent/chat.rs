use std::sync::{Arc, RwLock, Weak};

use crate::{
    agent::runtime::{ChatAgent, get_agent},
    chat::{ActiveWorkflow, EventChannel, PendingAction, workflow::Workflow},
    clients::SupportedModels,
    directive::{Directive, DirectiveSource, directive_path},
    tools::{AskQuestion, RecordThought, resolve_tools},
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
        event_channel: Weak<EventChannel<PendingAction>>,
    ) -> ChatAgent {
        let mut combined = vec![Directive {
            condition: None,
            source: DirectiveSource::File {
                paths: vec![directive_path("tenon_constitution.md")],
            },
        }];
        combined.extend(self.directive.iter().cloned());

        let mut tools = resolve_tools(&self.tool_names);

        // AskQuestion is a special system tool that is always resolved
        tools.insert(0, Box::new(AskQuestion { event_channel }));
        tools.insert(0, Box::new(RecordThought));

        let has_active = workflow_context
            .read()
            .map(|g| g.is_some())
            .unwrap_or(false);

        if has_active {
            use crate::tools::end_workflow::EndWorkflow;
            use crate::tools::navigate_workflow::NavigateWorkflow;
            tools.insert(
                0,
                Box::new(NavigateWorkflow {
                    active_workflow: workflow_context.clone(),
                }),
            );
            tools.insert(
                0,
                Box::new(EndWorkflow {
                    active_workflow: workflow_context,
                }),
            );
        } else if !self.workflows.is_empty() {
            use crate::tools::start_workflow::StartWorkflow;
            tools.insert(
                0,
                Box::new(StartWorkflow {
                    workflows: self.workflows.clone(),
                    active_workflow: workflow_context.clone(),
                }),
            );
        }

        get_agent(self.model.clone(), combined, tools, true)
    }
}
