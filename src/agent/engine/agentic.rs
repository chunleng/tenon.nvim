use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock, Weak};

use nvim_oxi::api::types::LogLevel;
use rig::completion::Usage;
use rig::message::ToolResultContent;
use rig::tool::DynamicTool;

use crate::agent::provider::{ChatStream, StreamItem, get_agent};
use crate::chat::prompt::build_choreo_prompt;
use crate::chat::{
    ActiveChoreo, ChatLogHandler, EventChannel, PendingAction, TenonAssistantMessage,
    TenonAssistantMessageContent, TenonChoreoLog, TenonLog, TenonLogData, TenonThoughtLog,
    TenonToolCall, TenonToolError, TenonToolLog, TenonToolResult, WorkQueue,
};
use crate::clients::SupportedModels;
use crate::directive::{Directive, DirectiveSource, directive_path};
use crate::tools::{AskQuestion, RecordThought, into_dynamic_tool};
use crate::utils::GLOBAL_EXECUTION_HANDLER;
use rig::agent::Agent;

/// Distinguishes agents with direct user access from sub-agents used as tools.
/// Determines which system tools (e.g. AskQuestion) are available.
pub enum AgenticAgentType {
    /// Agent has direct access to the user (main chat).
    Direct(Weak<EventChannel<PendingAction>>),
    /// Agent runs as a sub-tool without user interaction.
    Tool,
}

/// Streaming engine for agentic chat with tools, choreos, and multi-turn loops.
/// Session-state interfaces (log handler, usage, cancel token, etc.) are injected per request.
#[derive(Clone)]
pub struct AgenticStreamEngine {
    pub model: SupportedModels,
    pub directive: Vec<Directive>,
    pub tool_names: Vec<DynamicTool>,
    pub choreos: Vec<Arc<crate::chat::choreo::Choreo>>,
    pub active_choreo: Arc<RwLock<Option<ActiveChoreo>>>,
    pub work_queue: Arc<RwLock<WorkQueue>>,
    pub log_handler: ChatLogHandler,
    system_tools: Vec<DynamicTool>,
}

impl AgenticStreamEngine {
    pub fn new(
        model: SupportedModels,
        directive: Vec<Directive>,
        tool_names: Vec<DynamicTool>,
        choreos: Vec<Arc<crate::chat::choreo::Choreo>>,
        agent_type: AgenticAgentType,
    ) -> Self {
        let work_queue = Arc::new(RwLock::new(WorkQueue::default()));
        let mut system_tools = vec![into_dynamic_tool(RecordThought)];
        if let AgenticAgentType::Direct(event_channel) = agent_type {
            system_tools.insert(
                0,
                into_dynamic_tool(crate::tools::PushTasks {
                    work_queue: work_queue.clone(),
                }),
            );
            system_tools.insert(
                0,
                into_dynamic_tool(crate::tools::PopTask {
                    work_queue: work_queue.clone(),
                }),
            );
            system_tools.insert(0, into_dynamic_tool(AskQuestion { event_channel }));
        }
        Self {
            model,
            directive,
            tool_names,
            choreos,
            active_choreo: Arc::new(RwLock::new(None)),
            work_queue,
            log_handler: ChatLogHandler::new(),
            system_tools,
        }
    }

    /// Replaces log_window from logs and reconstructs active_choreo from choreo logs.
    pub fn load(&mut self, logs: Vec<TenonLog>) {
        self.log_handler.load(logs);

        let registry = crate::get_choreo_registry();
        let mut active: Option<ActiveChoreo> = None;
        {
            let log_window = self.log_handler.log_window.read().unwrap();
            for indexed in &log_window.logs {
                if let TenonLogData::Choreo(choreo_log) = indexed.log.data() {
                    match choreo_log.r#move {
                        Some(move_number) => {
                            if let Some(choreo) = registry.get(&choreo_log.id) {
                                active = Some(ActiveChoreo::new(choreo.clone(), move_number));
                            }
                        }
                        None => {
                            active = None;
                        }
                    }
                }
            }
        }
        *self.active_choreo.write().unwrap() = active;
    }

    fn build_chat_adapter(&self) -> Agent {
        let mut combined = vec![Directive {
            condition: None,
            source: DirectiveSource::Preset {
                id: "Tenon Constitution".into(),
                path: directive_path("tenon_constitution.md"),
            },
        }];
        combined.extend(self.directive.iter().cloned());

        // System tools must be resolved first
        let mut tools = self.system_tools.clone();
        tools.extend(self.tool_names.iter().cloned());

        let has_active = self
            .active_choreo
            .read()
            .map(|g| g.is_some())
            .unwrap_or(false);

        if has_active {
            use crate::tools::end_choreo::EndChoreo;
            use crate::tools::navigate_choreo::NavigateChoreo;
            tools.insert(
                0,
                into_dynamic_tool(NavigateChoreo {
                    active_choreo: self.active_choreo.clone(),
                }),
            );
            tools.insert(
                0,
                into_dynamic_tool(EndChoreo {
                    active_choreo: self.active_choreo.clone(),
                }),
            );
        } else if !self.choreos.is_empty() {
            use crate::tools::use_choreo::UseChoreo;
            tools.insert(
                0,
                into_dynamic_tool(UseChoreo {
                    choreos: self.choreos.clone(),
                    active_choreo: self.active_choreo.clone(),
                }),
            );
        }

        get_agent(self.model.clone(), combined, tools, None)
    }

    /// Process one turn of streaming chat.
    /// Text/Reasoning/ToolCall/ToolResult items are handled internally.
    /// `CompletionCall` is forwarded to `on_completion_call`; the caller owns
    /// usage tracking and history saving.
    /// Returns `true` when a choreo tool result is received (signal to continue
    /// the multi-turn loop), `false` otherwise.
    pub async fn process_turn(
        &mut self,
        prompt: String,
        cancel_token: &AtomicBool,
        on_completion_call: impl Fn(Usage),
        max_turns: usize,
    ) -> bool {
        let agent = self.build_chat_adapter();
        let chat_history = self.log_handler.get_chat_history(&prompt);
        let prompt = build_choreo_prompt(&self.active_choreo, &self.work_queue, prompt).await;
        let mut stream = ChatStream::new(&agent, prompt, chat_history, max_turns).await;

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
                                    id: tool_call.id.to_string(),
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
                            let result = match tool_result.content.first() {
                                Some(ToolResultContent::Text(text)) => {
                                    if text.text.starts_with("ToolCallError: ") {
                                        Err(TenonToolError(text.text.clone()))
                                    } else {
                                        Ok(TenonToolResult::Text(text.clone()))
                                    }
                                }
                                Some(ToolResultContent::Image(img)) => {
                                    Ok(TenonToolResult::Image(img.clone()))
                                }
                                Some(ToolResultContent::Json { value }) => {
                                    Ok(TenonToolResult::Text(rig::agent::Text {
                                        text: value.to_string(),
                                        ..Default::default()
                                    }))
                                }
                                None => Ok(TenonToolResult::Text(rig::agent::Text::default())),
                            };

                            log.set_tool_result(Some(result.clone()));

                            // Handle choreo tool results
                            if let TenonLogData::Tool(tool_log) = log.data()
                                && ["use_choreo", "navigate_choreo", "end_choreo"]
                                    .contains(&tool_log.tool_call.name.as_str())
                                && result.is_ok()
                            {
                                let tool_log_clone = tool_log.clone();

                                if tool_log_clone.tool_call.name == "end_choreo" {
                                    let id = self
                                        .active_choreo
                                        .read()
                                        .ok()
                                        .and_then(|active| {
                                            active.as_ref().map(|c| c.choreo.id.clone())
                                        })
                                        .unwrap_or_default();
                                    if let Ok(mut active) = self.active_choreo.write() {
                                        *active = None;
                                    }
                                    log_window.logs.push(crate::chat::log::indexer::IndexedLog {
                                        log: Arc::new(TenonLog::new(TenonLogData::Choreo(
                                            TenonChoreoLog::new(
                                                id,
                                                "Choreo ended",
                                                None,
                                                tool_log_clone,
                                            ),
                                        ))),
                                        active: true,
                                    });
                                } else if let Ok(active) = self.active_choreo.read()
                                    && let Some(active_choreo) = active.as_ref()
                                {
                                    let move_number = active_choreo.r#move;
                                    if let Ok(choreo_log) = active_choreo
                                        .choreo
                                        .generate_log(move_number, tool_log_clone)
                                    {
                                        log_window.logs.push(
                                            crate::chat::log::indexer::IndexedLog {
                                                log: Arc::new(TenonLog::new(TenonLogData::Choreo(
                                                    choreo_log,
                                                ))),
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
                            if let Some(ToolResultContent::Text(text)) = content {
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
                                                    id: tool_result.call.to_string(),
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
        let engine =
            AgenticStreamEngine::new(test_model(), vec![], tools, vec![], AgenticAgentType::Tool);
        let names: Vec<&str> = engine.tool_names.iter().map(|t| t.name()).collect();
        assert_eq!(names, vec!["read_file", "edit_file"]);
    }

    #[test]
    fn test_load_reconstructs_active_choreo_from_logs() {
        crate::utils::PLUGIN_ROOT
            .set(std::env::current_dir().unwrap())
            .ok();

        let tools = vec![into_dynamic_tool(ReadFile), into_dynamic_tool(EditFile)];
        let mut engine =
            AgenticStreamEngine::new(test_model(), vec![], tools, vec![], AgenticAgentType::Tool);

        // Navigate to move 2
        let move1_log = TenonLog::new(TenonLogData::Choreo(TenonChoreoLog {
            id: "find_software_bug_root_cause".to_string(),
            content: "Move 1".to_string(),
            r#move: Some(1),
            tool_log: TenonToolLog::default(),
        }));
        let move2_log = TenonLog::new(TenonLogData::Choreo(TenonChoreoLog {
            id: "find_software_bug_root_cause".to_string(),
            content: "Move 2".to_string(),
            r#move: Some(2),
            tool_log: TenonToolLog::default(),
        }));

        engine.load(vec![move1_log, move2_log]);

        {
            let active_choreo = engine.active_choreo.read().unwrap();
            assert!(active_choreo.is_some());
            assert_eq!(active_choreo.as_ref().unwrap().r#move, 2);
            assert_eq!(
                active_choreo.as_ref().unwrap().choreo.id,
                "find_software_bug_root_cause"
            );
        }

        // End choreo clears active_choreo
        let end_log = TenonLog::new(TenonLogData::Choreo(TenonChoreoLog {
            id: "find_software_bug_root_cause".to_string(),
            content: "End".to_string(),
            r#move: None,
            tool_log: TenonToolLog::default(),
        }));

        engine.load(vec![end_log]);

        {
            let active_choreo = engine.active_choreo.read().unwrap();
            assert!(active_choreo.is_none());
        }
    }
}
