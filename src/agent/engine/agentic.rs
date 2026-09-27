use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock, Weak};

use nvim_oxi::api::types::LogLevel;
use rig::completion::Usage;
use rig::message::ToolResultContent;
use rig::prelude::Message;
use rig::tool::{DynamicTool, ToolContext};

use crate::agent::provider::{ChatStream, StreamItem, get_agent};
use crate::chat::prompt::build_choreo_messages;
use crate::chat::{
    ActiveChoreo, ChatLogHandler, EventChannel, PendingAction, TenonAssistantMessage,
    TenonAssistantMessageContent, TenonLog, TenonLogData, TenonToolCall, TenonToolError,
    TenonToolLog, WorkQueue,
};
use crate::clients::SupportedModels;
use crate::directive::{Directive, DirectiveSource, PresetContent, directive_path};
use crate::tools::{AskQuestion, RecordThought, TenonTool, resolve_tools};
use crate::utils::GLOBAL_EXECUTION_HANDLER;
use rig::agent::Agent;

/// Signals the engine to end the current stream and start a new request.
/// Shared via `ToolContext` as `Arc<ContinueSignal>` so the flag survives
/// context clones; setters write it, `process_turn` reads and resets it.
pub struct ContinueSignal(pub AtomicBool);

/// Streaming engine for agentic chat with tools, choreos, and multi-turn loops.
/// Session-state interfaces (log handler, usage, cancel token, etc.) are injected per request.
#[derive(Clone)]
pub struct AgenticStreamEngine {
    pub model: SupportedModels,
    pub directive: Vec<Directive>,
    pub tools: Vec<DynamicTool>,
    pub choreos: Vec<Arc<crate::chat::choreo::Choreo>>,
    pub active_choreo: Arc<RwLock<Option<ActiveChoreo>>>,
    pub log_handler: ChatLogHandler,
    pub system_tools: Vec<DynamicTool>,
    /// Session state passed to tools each turn
    pub tool_context: ToolContext,
}

impl AgenticStreamEngine {
    pub fn new(
        model: SupportedModels,
        directive: Vec<Directive>,
        tool_names: Vec<String>,
        choreos: Vec<Arc<crate::chat::choreo::Choreo>>,
        mut tool_context: ToolContext,
    ) -> Self {
        let log_handler = ChatLogHandler::new();
        let mut system_tools =
            vec![TenonTool::new(RecordThought, log_handler.log_window.clone()).into()];

        // Allow tools to set this flag to indicate to chat to restart a new stream so that we can
        // inject context base on the new state. Useful for tools that changes chat session's state
        // (e.g. Choreo tools)
        tool_context.insert(Arc::new(ContinueSignal(AtomicBool::new(false))));

        if tool_context.contains::<Arc<RwLock<WorkQueue>>>() {
            system_tools.insert(
                0,
                TenonTool::new(crate::tools::PushTasks, log_handler.log_window.clone()).into(),
            );
            system_tools.insert(
                0,
                TenonTool::new(crate::tools::PopTask, log_handler.log_window.clone()).into(),
            );
        }

        if tool_context.contains::<Weak<EventChannel<PendingAction>>>() {
            system_tools.insert(
                0,
                TenonTool::new(AskQuestion, log_handler.log_window.clone()).into(),
            );
        }
        let tools = resolve_tools(&tool_names, log_handler.log_window.clone());
        Self {
            model,
            directive,
            tools,
            choreos,
            active_choreo: Arc::new(RwLock::new(None)),
            log_handler,
            system_tools,
            tool_context,
        }
    }

    /// Re-resolves tools from raw selectors, replacing the current set.
    pub fn set_tools(&mut self, tool_names: Vec<String>) {
        self.tools = resolve_tools(&tool_names, self.log_handler.log_window.clone());
    }

    /// Replaces log_window from logs and reconstructs active_choreo from choreo logs.
    pub fn load(&mut self, logs: Vec<TenonLog>) {
        self.log_handler.load(logs);

        let registry = crate::get_choreo_registry();
        let mut active: Option<ActiveChoreo> = None;
        {
            let log_window = self.log_handler.log_window.read().unwrap();
            for indexed in &log_window.logs {
                let Ok(log) = indexed.log.read() else {
                    continue;
                };
                if let TenonLogData::Choreo(choreo_log) = log.data() {
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
        if let Ok(mut active_choreo) = self.active_choreo.write() {
            *active_choreo = active;
        }
    }

    fn build_chat_adapter(&self) -> Agent {
        let mut combined = vec![Directive {
            condition: None,
            source: DirectiveSource::Preset {
                id: "Tenon Constitution".into(),
                content: PresetContent::File(directive_path("tenon_constitution.md")),
            },
        }];
        combined.extend(self.directive.iter().cloned());

        // System tools must be resolved first
        let mut tools = self.system_tools.clone();
        tools.extend(self.tools.clone());

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
                TenonTool::new(
                    NavigateChoreo {
                        active_choreo: self.active_choreo.clone(),
                    },
                    self.log_handler.log_window.clone(),
                )
                .into(),
            );
            tools.insert(
                0,
                TenonTool::new(
                    EndChoreo {
                        active_choreo: self.active_choreo.clone(),
                    },
                    self.log_handler.log_window.clone(),
                )
                .into(),
            );
        } else if !self.choreos.is_empty() {
            use crate::tools::use_choreo::UseChoreo;
            tools.insert(
                0,
                TenonTool::new(
                    UseChoreo {
                        choreos: self.choreos.clone(),
                        active_choreo: self.active_choreo.clone(),
                    },
                    self.log_handler.log_window.clone(),
                )
                .into(),
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
        let mut chat_history = self.log_handler.get_chat_history(&prompt);
        let mut messages = build_choreo_messages(
            &self.active_choreo,
            self.tool_context.get::<Arc<RwLock<WorkQueue>>>(),
            prompt,
        )
        .await;
        let message = if messages.is_empty() {
            Message::system("<context></context>")
        } else {
            let message = messages.pop().unwrap();
            chat_history.extend(messages);
            message
        };
        let mut stream = ChatStream::new(
            &agent,
            message,
            chat_history,
            max_turns,
            self.tool_context.clone(),
        )
        .await;

        while let Some(result) = stream.next().await {
            if cancel_token.load(Ordering::SeqCst) {
                break;
            }
            match result {
                Ok(StreamItem::Text { text }) => {
                    if let Ok(mut log_window) = self.log_handler.log_window.write() {
                        let mut updated = false;
                        if let Some(indexed_log) = log_window.logs.last_mut()
                            && let Ok(mut log) = indexed_log.log.write()
                        {
                            updated = log.append_text(&text);
                        }
                        if !updated {
                            log_window.logs.push(crate::chat::log::indexer::IndexedLog {
                                log: Arc::new(RwLock::new(TenonLog::new(TenonLogData::Assistant(
                                    TenonAssistantMessage {
                                        reasoning: None,
                                        content: vec![TenonAssistantMessageContent::Text(text)],
                                    },
                                )))),
                                active: true,
                            });
                        }
                    }
                }
                Ok(StreamItem::ReasoningDelta { reasoning }) => {
                    if let Ok(mut log_window) = self.log_handler.log_window.write() {
                        let mut updated = false;
                        if let Some(indexed_log) = log_window.logs.last_mut()
                            && let Ok(mut log) = indexed_log.log.write()
                        {
                            updated = log.append_reasoning(&reasoning);
                        }
                        if !updated {
                            log_window.logs.push(crate::chat::log::indexer::IndexedLog {
                                log: Arc::new(RwLock::new(TenonLog::new(TenonLogData::Assistant(
                                    TenonAssistantMessage {
                                        reasoning: Some(reasoning),
                                        content: vec![],
                                    },
                                )))),
                                active: true,
                            });
                        }
                    }
                }
                Ok(StreamItem::ToolCall { .. }) => {}
                Ok(StreamItem::ToolResult {
                    tool_result,
                    internal_call_id,
                }) => {
                    // A choreo tool set the flag: end this stream, start a new request
                    if let Some(signal) = self.tool_context.get::<Arc<ContinueSignal>>()
                        && signal.0.load(Ordering::SeqCst)
                    {
                        break;
                    }

                    if let Ok(mut log_window) = self.log_handler.log_window.write() {
                        // No matching Tool log → the tool call was invalid and skipped by
                        // InvalidToolCallHook.
                        let content = tool_result.content.first();
                        if let Some(ToolResultContent::Text(text)) = content
                            && text.text.starts_with("ToolCallError: ")
                        {
                            // Fake the tool call so the log is displayable and produces valid
                            // LLM history (matching tool_call.id + tool_result.id).
                            let tool_name = text
                                .text
                                .strip_prefix("ToolCallError: `")
                                .and_then(|s| s.split('`').next())
                                .unwrap_or("unknown")
                                .to_string();
                            log_window.logs.push(crate::chat::log::indexer::IndexedLog {
                                log: Arc::new(RwLock::new(TenonLog::new(TenonLogData::Tool(
                                    TenonToolLog {
                                        tool_call: TenonToolCall {
                                            id: tool_result.call.to_string(),
                                            item_id: tool_result
                                                .provider
                                                .as_ref()
                                                .and_then(|p| p.item_id.clone()),
                                            internal_call_id: internal_call_id.clone(),
                                            name: tool_name,
                                            args: serde_json::Value::Null,
                                        },
                                        tool_result: Some(Err(TenonToolError(text.text.clone()))),
                                        progress: vec![],
                                    },
                                )))),
                                active: true,
                            });
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

        self.tool_context
            .get::<Arc<ContinueSignal>>()
            .is_some_and(|signal| signal.0.swap(false, Ordering::SeqCst))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chat::TenonChoreoLog;
    use crate::clients::{OllamaProviderConfig, ProviderConfig, SupportedModels};

    fn test_model() -> SupportedModels {
        SupportedModels {
            connector_name: "test".to_string(),
            config: ProviderConfig::Ollama(OllamaProviderConfig::default()),
            model_name: "test".to_string(),
            default_parameters: serde_json::Map::new(),
        }
    }

    #[test]
    fn test_engine_resolves_tools_at_new() {
        let engine = AgenticStreamEngine::new(
            test_model(),
            vec![],
            vec!["read_file".to_string(), "edit_file".to_string()],
            vec![],
            ToolContext::new(),
        );
        let names: Vec<String> = engine.tools.iter().map(|t| t.name().to_string()).collect();
        assert_eq!(names, vec!["read_file", "edit_file"]);
    }

    #[test]
    fn test_load_reconstructs_active_choreo_from_logs() {
        crate::utils::PLUGIN_ROOT
            .set(std::env::current_dir().unwrap())
            .ok();

        let mut engine = AgenticStreamEngine::new(
            test_model(),
            vec![],
            vec!["read_file".to_string(), "edit_file".to_string()],
            vec![],
            ToolContext::new(),
        );

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
