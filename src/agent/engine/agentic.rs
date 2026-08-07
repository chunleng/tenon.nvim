use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock, Weak};

use nvim_oxi::api::types::LogLevel;
use rig::completion::Usage;
use rig::message::ToolResultContent;
use rig::tool::DynamicTool;

use crate::agent::provider::{ChatAgent, StreamItem, get_agent};
use crate::chat::prompt::build_workflow_prompt;
use crate::chat::{
    ActiveWorkflow, ChatLogHandler, EventChannel, PendingAction, TenonAssistantMessage,
    TenonAssistantMessageContent, TenonLog, TenonLogData, TenonThoughtLog, TenonToolCall,
    TenonToolError, TenonToolLog, TenonToolResult, TenonWorkflowLog,
};
use crate::clients::SupportedModels;
use crate::directive::{Directive, DirectiveSource, directive_path};
use crate::tools::{AskQuestion, RecordThought, into_dynamic_tool};
use crate::utils::GLOBAL_EXECUTION_HANDLER;

/// Streaming engine for agentic chat with tools, workflows, and multi-turn loops.
/// Session-state interfaces (log handler, usage, cancel token, etc.) are injected per request.
#[derive(Clone)]
pub struct AgenticStreamEngine {
    pub model: SupportedModels,
    pub directive: Vec<Directive>,
    pub tool_names: Vec<DynamicTool>,
    pub workflows: Vec<Arc<crate::chat::workflow::Workflow>>,
    pub active_workflow: Arc<RwLock<Option<ActiveWorkflow>>>,
    pub log_handler: ChatLogHandler,
    ask_question_tool: DynamicTool,
}

impl AgenticStreamEngine {
    pub fn new(
        model: SupportedModels,
        directive: Vec<Directive>,
        tool_names: Vec<DynamicTool>,
        workflows: Vec<Arc<crate::chat::workflow::Workflow>>,
        event_channel: Weak<EventChannel<PendingAction>>,
    ) -> Self {
        let ask_question_tool = into_dynamic_tool(AskQuestion { event_channel });
        Self {
            model,
            directive,
            tool_names,
            workflows,
            active_workflow: Arc::new(RwLock::new(None)),
            log_handler: ChatLogHandler::new(),
            ask_question_tool,
        }
    }

    /// Replaces log_window from logs and reconstructs active_workflow from workflow logs.
    pub fn load(&mut self, logs: Vec<TenonLog>) {
        self.log_handler.load(logs);

        let registry = crate::get_workflow_registry();
        let mut wf: Option<ActiveWorkflow> = None;
        {
            let log_window = self.log_handler.log_window.read().unwrap();
            for indexed in &log_window.logs {
                if let TenonLogData::Workflow(wf_log) = indexed.log.data() {
                    match wf_log.step {
                        Some(step) => {
                            if let Some(workflow) = registry.get(&wf_log.id) {
                                wf = Some(ActiveWorkflow::new(workflow.clone(), step));
                            }
                        }
                        None => {
                            wf = None;
                        }
                    }
                }
            }
        }
        *self.active_workflow.write().unwrap() = wf;
    }

    fn build_chat_adapter(&self) -> ChatAgent {
        let mut combined = vec![Directive {
            condition: None,
            source: DirectiveSource::File {
                paths: vec![directive_path("tenon_constitution.md")],
            },
        }];
        combined.extend(self.directive.iter().cloned());

        let mut tools = self.tool_names.clone();

        // Prebuilt tools (e.g. AskQuestion) are always resolved
        tools.insert(0, self.ask_question_tool.clone());
        tools.insert(0, into_dynamic_tool(RecordThought));

        let has_active = self
            .active_workflow
            .read()
            .map(|g| g.is_some())
            .unwrap_or(false);

        if has_active {
            use crate::tools::end_workflow::EndWorkflow;
            use crate::tools::navigate_workflow::NavigateWorkflow;
            tools.insert(
                0,
                into_dynamic_tool(NavigateWorkflow {
                    active_workflow: self.active_workflow.clone(),
                }),
            );
            tools.insert(
                0,
                into_dynamic_tool(EndWorkflow {
                    active_workflow: self.active_workflow.clone(),
                }),
            );
        } else if !self.workflows.is_empty() {
            use crate::tools::start_workflow::StartWorkflow;
            tools.insert(
                0,
                into_dynamic_tool(StartWorkflow {
                    workflows: self.workflows.clone(),
                    active_workflow: self.active_workflow.clone(),
                }),
            );
        }

        get_agent(self.model.clone(), combined, tools, None)
    }

    /// Process one turn of streaming chat.
    /// Text/Reasoning/ToolCall/ToolResult items are handled internally.
    /// `CompletionCall` is forwarded to `on_completion_call`; the caller owns
    /// usage tracking and history saving.
    /// Returns `true` when a workflow tool result is received (signal to continue
    /// the multi-turn loop), `false` otherwise.
    pub async fn process_turn(
        &mut self,
        cancel_token: &AtomicBool,
        on_completion_call: impl Fn(Usage),
    ) -> bool {
        let agent = self.build_chat_adapter();
        let next_prompt = self.log_handler.get_user_prompt();
        let chat_history = self.log_handler.get_chat_history(&next_prompt);
        let prompt = build_workflow_prompt(
            &self.active_workflow,
            &self.workflows,
            &self.model,
            next_prompt,
        )
        .await;
        let mut stream = agent.stream_chat(prompt, chat_history).await;

        let mut should_continue = false;

        while let Some(result) = stream.next().await {
            if cancel_token.load(Ordering::SeqCst) {
                break;
            }
            match result {
                Ok(StreamItem::Text { text }) => {
                    if let Ok(mut log_window) = self.log_handler.log_window.write() {
                        let mut updated = false;
                        if let Some(indexed_log) = log_window.logs.last_mut() {
                            let log = Arc::make_mut(&mut indexed_log.log);
                            updated = log.append_text(&text);
                        }
                        if !updated {
                            log_window.logs.push(crate::chat::log::indexer::IndexedLog {
                                log: Arc::new(TenonLog::new(TenonLogData::Assistant(
                                    TenonAssistantMessage {
                                        reasoning: None,
                                        content: vec![TenonAssistantMessageContent::Text(text)],
                                    },
                                ))),
                                active: true,
                            });
                        }
                    }
                }
                Ok(StreamItem::ReasoningDelta { reasoning }) => {
                    if let Ok(mut log_window) = self.log_handler.log_window.write() {
                        let mut updated = false;
                        if let Some(indexed_log) = log_window.logs.last_mut() {
                            let log = Arc::make_mut(&mut indexed_log.log);
                            updated = log.append_reasoning(&reasoning);
                        }
                        if !updated {
                            log_window.logs.push(crate::chat::log::indexer::IndexedLog {
                                log: Arc::new(TenonLog::new(TenonLogData::Assistant(
                                    TenonAssistantMessage {
                                        reasoning: Some(reasoning),
                                        content: vec![],
                                    },
                                ))),
                                active: true,
                            });
                        }
                    }
                }
                Ok(StreamItem::ToolCall {
                    tool_call,
                    internal_call_id,
                }) => {
                    if tool_call.function.name != "record_thought"
                        && let Ok(mut log_window) = self.log_handler.log_window.write()
                    {
                        log_window.logs.push(crate::chat::log::indexer::IndexedLog {
                            log: Arc::new(TenonLog::new(TenonLogData::Tool(TenonToolLog {
                                tool_call: TenonToolCall {
                                    id: tool_call.id,
                                    internal_call_id,
                                    name: tool_call.function.name,
                                    args: tool_call.function.arguments,
                                },
                                tool_result: None,
                            }))),
                            active: true,
                        });
                    }
                }
                Ok(StreamItem::ToolResult {
                    tool_result,
                    internal_call_id,
                }) => {
                    if let Ok(mut log_window) = self.log_handler.log_window.write() {
                        if let Some(log) = log_window.logs.iter_mut().find_map(|x| {
                            if let TenonLogData::Tool(tool) = x.log.data()
                                && tool.tool_call.internal_call_id == internal_call_id
                            {
                                return Some(x);
                            }
                            None
                        }) {
                            let log = Arc::make_mut(&mut log.log);
                            let tool_result = tool_result.content.first();
                            let result = match tool_result {
                                ToolResultContent::Text(text) => {
                                    if text.text.starts_with("ToolCallError: ") {
                                        Err(TenonToolError(text.text))
                                    } else {
                                        Ok(TenonToolResult::Text(text))
                                    }
                                }
                                ToolResultContent::Image(img) => Ok(TenonToolResult::Image(img)),
                                ToolResultContent::Json { value } => {
                                    Ok(TenonToolResult::Text(rig::agent::Text {
                                        text: value.to_string(),
                                        ..Default::default()
                                    }))
                                }
                            };

                            log.set_tool_result(Some(result.clone()));

                            // Handle workflow tool results
                            if let TenonLogData::Tool(tool_log) = log.data()
                                && ["start_workflow", "navigate_workflow", "end_workflow"]
                                    .contains(&tool_log.tool_call.name.as_str())
                                && result.is_ok()
                            {
                                let tool_log_clone = tool_log.clone();

                                if tool_log_clone.tool_call.name == "end_workflow" {
                                    let id = self
                                        .active_workflow
                                        .read()
                                        .ok()
                                        .and_then(|active| {
                                            active.as_ref().map(|wf| wf.workflow.id.clone())
                                        })
                                        .unwrap_or_default();
                                    if let Ok(mut active) = self.active_workflow.write() {
                                        *active = None;
                                    }
                                    log_window.logs.push(crate::chat::log::indexer::IndexedLog {
                                        log: Arc::new(TenonLog::new(TenonLogData::Workflow(
                                            TenonWorkflowLog::new(
                                                id,
                                                "Workflow ended",
                                                None,
                                                tool_log_clone,
                                            ),
                                        ))),
                                        active: true,
                                    });
                                } else if let Ok(active) = self.active_workflow.read()
                                    && let Some(active_wf) = active.as_ref()
                                {
                                    let step = active_wf.step;
                                    if let Ok(wf_log) =
                                        active_wf.workflow.generate_log(step, tool_log_clone)
                                    {
                                        log_window.logs.push(
                                            crate::chat::log::indexer::IndexedLog {
                                                log: Arc::new(TenonLog::new(
                                                    TenonLogData::Workflow(wf_log),
                                                )),
                                                active: true,
                                            },
                                        );
                                    }
                                }

                                should_continue = true;
                                break;
                            }
                        } else {
                            // No matching Tool log → record_thought result.
                            // The tool returns JSON: {"thought": "...", "summary": null|"..."}
                            let content = tool_result.content.first();
                            if let ToolResultContent::Text(text) = content {
                                if text.text.starts_with("ToolCallError: ") {
                                    // Invalid tool call was skipped by
                                    // InvalidToolCallHook — no preceding ToolCall
                                    // stream item arrived. Fake the tool call so the
                                    // log is displayable and produces valid LLM
                                    // history (matching tool_call.id + tool_result.id).
                                    let tool_name = text
                                        .text
                                        .strip_prefix("ToolCallError: `")
                                        .and_then(|s| s.split('`').next())
                                        .unwrap_or("unknown")
                                        .to_string();
                                    log_window.logs.push(crate::chat::log::indexer::IndexedLog {
                                        log: Arc::new(TenonLog::new(TenonLogData::Tool(
                                            TenonToolLog {
                                                tool_call: TenonToolCall {
                                                    id: tool_result.id.clone(),
                                                    internal_call_id: internal_call_id.clone(),
                                                    name: tool_name,
                                                    args: serde_json::Value::Null,
                                                },
                                                tool_result: Some(Err(TenonToolError(
                                                    text.text.clone(),
                                                ))),
                                            },
                                        ))),
                                        active: true,
                                    });
                                } else {
                                    if let Ok(parsed) =
                                        serde_json::from_str::<serde_json::Value>(&text.text)
                                    {
                                        let thought = parsed
                                            .get("thought")
                                            .and_then(|v| v.as_str())
                                            .unwrap_or_default()
                                            .to_string();
                                        let summary = parsed
                                            .get("summary")
                                            .and_then(|v| v.as_str())
                                            .map(|s| s.to_string());
                                        log_window.logs.push(
                                            crate::chat::log::indexer::IndexedLog {
                                                log: Arc::new(TenonLog::new(
                                                    TenonLogData::Thought(TenonThoughtLog {
                                                        thought,
                                                        summary,
                                                    }),
                                                )),
                                                active: true,
                                            },
                                        );
                                    }
                                }
                            }
                        }
                    }
                }
                Ok(StreamItem::Other) => {}
                Ok(StreamItem::CompletionCall { usage }) => {
                    on_completion_call(usage);
                }
                Err(e) => {
                    GLOBAL_EXECUTION_HANDLER.notify_on_main_thread(
                        format!("error occurred while streaming response from LLM: {}", e),
                        LogLevel::Error,
                    );
                }
            }
        }

        should_continue
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chat::EventChannel;
    use crate::clients::{OllamaProviderConfig, ProviderConfig, SupportedModels};
    use crate::tools::{EditFile, ReadFile};

    fn test_model() -> SupportedModels {
        SupportedModels {
            connector_name: "test".to_string(),
            config: ProviderConfig::Ollama(OllamaProviderConfig::default()),
            model_name: "test".to_string(),
            default_parameters: serde_json::Map::new(),
        }
    }

    #[test]
    fn test_engine_stores_dynamic_tool_names() {
        let tools = vec![into_dynamic_tool(ReadFile), into_dynamic_tool(EditFile)];
        let event_channel = Arc::new(EventChannel::new());
        let engine = AgenticStreamEngine::new(
            test_model(),
            vec![],
            tools,
            vec![],
            Arc::downgrade(&event_channel),
        );
        let names: Vec<&str> = engine.tool_names.iter().map(|t| t.name()).collect();
        assert_eq!(names, vec!["read_file", "edit_file"]);
    }

    #[test]
    fn test_load_reconstructs_active_workflow_from_logs() {
        crate::utils::PLUGIN_ROOT
            .set(std::env::current_dir().unwrap())
            .ok();

        let tools = vec![into_dynamic_tool(ReadFile), into_dynamic_tool(EditFile)];
        let event_channel = Arc::new(EventChannel::new());
        let mut engine = AgenticStreamEngine::new(
            test_model(),
            vec![],
            tools,
            vec![],
            Arc::downgrade(&event_channel),
        );

        // Navigate to step 2
        let step1_log = TenonLog::new(TenonLogData::Workflow(TenonWorkflowLog {
            id: "find_software_bug_root_cause".to_string(),
            content: "Step 1".to_string(),
            step: Some(1),
            tool_log: TenonToolLog::default(),
        }));
        let step2_log = TenonLog::new(TenonLogData::Workflow(TenonWorkflowLog {
            id: "find_software_bug_root_cause".to_string(),
            content: "Step 2".to_string(),
            step: Some(2),
            tool_log: TenonToolLog::default(),
        }));

        engine.load(vec![step1_log, step2_log]);

        {
            let active_workflow = engine.active_workflow.read().unwrap();
            assert!(active_workflow.is_some());
            assert_eq!(active_workflow.as_ref().unwrap().step, 2);
            assert_eq!(
                active_workflow.as_ref().unwrap().workflow.id,
                "find_software_bug_root_cause"
            );
        }

        // End workflow clears active_workflow
        let end_log = TenonLog::new(TenonLogData::Workflow(TenonWorkflowLog {
            id: "find_software_bug_root_cause".to_string(),
            content: "End".to_string(),
            step: None,
            tool_log: TenonToolLog::default(),
        }));

        engine.load(vec![end_log]);

        {
            let active_workflow = engine.active_workflow.read().unwrap();
            assert!(active_workflow.is_none());
        }
    }
}
