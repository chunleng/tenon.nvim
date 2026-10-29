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
use crate::tools::{AskQuestion, TenonTool};
use crate::utils::GLOBAL_EXECUTION_HANDLER;
use rig::agent::Agent;

/// Signals the engine to end the current stream and start a new request.
/// Shared via `ToolContext` as `Arc<ContinueSignal>` so the flag survives
/// context clones; setters write it, `process_turn` reads and resets it.
pub struct ContinueSignal(pub AtomicBool);

/// Enforcement keyword that must never reach the user in assistant chat.
const TENON_CHAT_KEYWORD: &str = "::tenon-chat::";

/// Realtime filter that drops `::tenon-chat::` from streamed assistant text. Only line-start
/// occurrences (message start or after a newline) are filtered; mid-line occurrences are legitimate
/// content and pass through.
enum ChatKeywordFilter {
    /// At a line start, accumulating text whose fate is not yet decidable.
    Checking(String),
    /// Mid-line: no match possible until the next line start.
    MidLine,
}

impl Default for ChatKeywordFilter {
    fn default() -> Self {
        Self::Checking(String::new())
    }
}

/// Strips the keyword and one following space, returning the remainder.
fn keyword_remainder(line: &str) -> Option<&str> {
    line.strip_prefix(TENON_CHAT_KEYWORD)
        .map(|remainder| remainder.strip_prefix(' ').unwrap_or(remainder))
}

impl ChatKeywordFilter {
    fn new() -> Self {
        Self::default()
    }

    /// Consumes streamed text and returns the part that is safe to display.
    fn feed(&mut self, text: &str) -> String {
        let mut out = String::new();
        let mut rest = text;
        while !rest.is_empty() {
            // Take the state out so every branch can reassign it freely.
            match std::mem::take(self) {
                Self::Checking(mut held) => match rest.find('\n') {
                    Some(pos) => {
                        // A newline-terminated line's fate is decidable now.
                        held.push_str(&rest[..pos]);
                        rest = &rest[pos + 1..];
                        let line = std::mem::take(&mut held);
                        match keyword_remainder(&line) {
                            Some(remainder) => out.push_str(remainder),
                            None => out.push_str(&line),
                        }
                        out.push('\n');
                        // The next character starts a new line to check.
                        *self = Self::Checking(held);
                    }
                    None => {
                        // No newline yet: the line may still become the keyword, so hold and wait
                        // for more text.
                        held.push_str(rest);
                        rest = "";
                        if TENON_CHAT_KEYWORD.starts_with(held.as_str()) {
                            // Still a potential prefix (including the
                            // complete keyword): keep holding.
                            *self = Self::Checking(held);
                        } else if let Some(remainder) = keyword_remainder(&held) {
                            out.push_str(remainder);
                            *self = Self::MidLine;
                        } else {
                            out.push_str(&held);
                            *self = Self::MidLine;
                        }
                    }
                },
                Self::MidLine => {
                    // Mid-line text is safe up to the first newline; after it a
                    // new line starts and must be checked.
                    match rest.find('\n') {
                        Some(pos) => {
                            let (safe, pending) = rest.split_at(pos + 1);
                            out.push_str(safe);
                            *self = Self::Checking(String::new());
                            rest = pending;
                        }
                        None => {
                            out.push_str(rest);
                            rest = "";
                            *self = Self::MidLine;
                        }
                    }
                }
            }
        }
        out
    }

    /// Resolves text still held when the stream ends: a confirmed keyword is dropped, anything else
    /// is released.
    fn flush(&mut self) -> String {
        match std::mem::take(self) {
            Self::Checking(held) => match keyword_remainder(&held) {
                Some(remainder) => remainder.to_string(),
                None => held,
            },
            Self::MidLine => String::new(),
        }
    }
}

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
        tools: Vec<DynamicTool>,
        choreos: Vec<Arc<crate::chat::choreo::Choreo>>,
        tool_context: ToolContext,
    ) -> Self {
        Self::with_log_handler(
            model,
            directive,
            tools,
            choreos,
            tool_context,
            ChatLogHandler::new(),
        )
    }

    pub fn with_log_handler(
        model: SupportedModels,
        directive: Vec<Directive>,
        tools: Vec<DynamicTool>,
        choreos: Vec<Arc<crate::chat::choreo::Choreo>>,
        mut tool_context: ToolContext,
        log_handler: ChatLogHandler,
    ) -> Self {
        let mut system_tools: Vec<DynamicTool> = Vec::new();

        // Allow tools to set this flag to indicate to chat to restart a new stream so that we can
        // inject context base on the new state. Useful for tools that changes chat session's state
        // (e.g. Choreo tools)
        tool_context = tool_context.with_scope(Arc::new(ContinueSignal(AtomicBool::new(false))));

        if tool_context.scope::<RwLock<WorkQueue>>().is_some() {
            system_tools.insert(
                0,
                TenonTool::new(crate::tools::PushTasks, log_handler.log_window.clone()).into(),
            );
            system_tools.insert(
                0,
                TenonTool::new(crate::tools::PopTask, log_handler.log_window.clone()).into(),
            );
        }

        if tool_context
            .scope::<Weak<EventChannel<PendingAction>>>()
            .is_some()
        {
            system_tools.insert(
                0,
                TenonTool::new(AskQuestion, log_handler.log_window.clone()).into(),
            );
        }

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

    /// Replaces the current tool set with caller-resolved tools.
    pub fn set_tools(&mut self, tools: Vec<DynamicTool>) {
        self.tools = tools;
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

    /// Appends streamed assistant text to the log, creating a new Assistant
    /// log when the last log cannot be appended to. Empty text is skipped so
    /// no empty log is created.
    fn append_stream_text(&mut self, text: &str) {
        if text.is_empty() {
            return;
        }
        if let Ok(mut log_window) = self.log_handler.log_window.write() {
            let mut updated = false;
            if let Some(indexed_log) = log_window.logs.last_mut()
                && let Ok(mut log) = indexed_log.log.write()
            {
                updated = log.append_text(text);
            }
            if !updated {
                log_window.add_log(crate::chat::log::indexer::IndexedLog {
                    log: Arc::new(RwLock::new(TenonLog::new(TenonLogData::Assistant(
                        TenonAssistantMessage {
                            reasoning: None,
                            content: vec![TenonAssistantMessageContent::Text(text.to_string())],
                            complete: false,
                        },
                    )))),
                    active: true,
                });
            }
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
        if self.model.vision {
            tools.push(
                TenonTool::new(
                    crate::tools::ReadImage::Vision {
                        model: self.model.clone(),
                    },
                    self.log_handler.log_window.clone(),
                )
                .into(),
            );
        } else if let Some(fallback_model) = crate::get_application_config()
            .tools
            .read_image
            .fallback_model
            .clone()
        {
            tools.push(
                TenonTool::new(
                    crate::tools::ReadImage::NonVision { fallback_model },
                    self.log_handler.log_window.clone(),
                )
                .into(),
            );
        }
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
        const MAX_STREAM_ATTEMPTS: usize = 3;

        'attempts: for attempt in 0..MAX_STREAM_ATTEMPTS {
            // Filter state is per attempt: a retry prunes the incomplete
            // messages, so held text from a failed attempt is discarded.
            let mut keyword_filter = ChatKeywordFilter::new();
            let agent = self.build_chat_adapter();
            let mut chat_history = self.log_handler.get_chat_history(&prompt);
            let mut messages = build_choreo_messages(
                &self.active_choreo,
                self.tool_context.scope::<RwLock<WorkQueue>>().as_ref(),
                prompt.clone(),
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
            );

            while let Some(result) = stream.next().await {
                if cancel_token.load(Ordering::SeqCst) {
                    break;
                }
                match result {
                    Ok(StreamItem::Text { text }) => {
                        let text = keyword_filter.feed(&text);
                        self.append_stream_text(&text);
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
                                log_window.add_log(crate::chat::log::indexer::IndexedLog {
                                    log: Arc::new(RwLock::new(TenonLog::new(
                                        TenonLogData::Assistant(TenonAssistantMessage {
                                            reasoning: Some(reasoning),
                                            content: vec![],
                                            complete: false,
                                        }),
                                    ))),
                                    active: true,
                                });
                            }
                        }
                    }
                    Ok(StreamItem::ToolCall { .. }) => {}
                    Ok(StreamItem::ToolResult { tool_result }) => {
                        // A choreo tool set the flag: end this stream, start a new request
                        if let Some(signal) = self.tool_context.scope::<ContinueSignal>()
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
                                let call_id = tool_result.call.to_string();
                                let mut fake_log =
                                    TenonLog::new(TenonLogData::Tool(TenonToolLog {
                                        tool_call: TenonToolCall {
                                            id: call_id.clone(),
                                            // rig 0.44 collapsed the dual-wire call id
                                            // (item handle + correlator) into one, so
                                            // there is no separate item id to record.
                                            item_id: None,
                                            internal_call_id: call_id,
                                            name: tool_name,
                                            args: serde_json::Value::Null,
                                        },
                                        tool_result: Some(Err(TenonToolError(text.text.clone()))),
                                        progress: vec![],
                                    }));
                                // The tool never ran, so its properties are undeterminable
                                fake_log.properties = crate::chat::log::LogProperties::UNKNOWN;
                                log_window.add_log(crate::chat::log::indexer::IndexedLog {
                                    log: Arc::new(RwLock::new(fake_log)),
                                    active: true,
                                });
                            }
                        }
                    }
                    Ok(StreamItem::Other) => {}
                    Ok(StreamItem::CompletionCall { usage }) => {
                        on_completion_call(usage);
                        if let Ok(mut log_window) = self.log_handler.log_window.write()
                            && let Some(indexed_log) = log_window.logs.last_mut()
                            && let Ok(mut log) = indexed_log.log.write()
                            && let TenonLogData::Assistant(message) = &mut log.data
                        {
                            message.complete = true;
                        }
                    }
                    Err(e) => {
                        // Mid-stream failures (e.g. "error decoding response body") surface here
                        // after partial content was delivered;
                        if attempt + 1 < MAX_STREAM_ATTEMPTS
                            && let Ok(mut log_window) = self.log_handler.log_window.write()
                        {
                            log_window.prune_incomplete_messages();
                            continue 'attempts;
                        }
                        GLOBAL_EXECUTION_HANDLER.notify_on_main_thread(
                            format!("error occurred while streaming response from LLM: {}", e),
                            LogLevel::Error,
                        );
                    }
                }
            }

            // The stream ended: resolve whatever the filter still holds.
            let flushed = keyword_filter.flush();
            self.append_stream_text(&flushed);

            break;
        }

        self.tool_context
            .scope::<ContinueSignal>()
            .is_some_and(|signal| signal.0.swap(false, Ordering::SeqCst))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chat::TenonChoreoLog;
    use crate::clients::{OllamaProviderConfig, ProviderConfig, ProviderKind, SupportedModels};
    use crate::tools::resolve_tools;
    use std::net::SocketAddr;
    use std::panic::AssertUnwindSafe;
    use std::sync::atomic::AtomicUsize;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::{TcpListener, TcpStream};

    use futures::FutureExt;

    fn test_model() -> SupportedModels {
        SupportedModels {
            connector_name: "test".to_string(),
            config: ProviderConfig {
                kind: ProviderKind::Ollama(OllamaProviderConfig::default()),
            },
            model_name: "test".to_string(),
            default_parameters: serde_json::Map::new(),
            vision: false,
        }
    }

    #[tokio::test]
    async fn test_engine_uses_caller_resolved_tools() {
        // The caller resolves tool names into DynamicTools before passing them in.
        let log_handler = ChatLogHandler::new();
        let tools = resolve_tools(
            &["read_file".to_string(), "edit_file".to_string()],
            log_handler.log_window.clone(),
        );
        let engine =
            AgenticStreamEngine::new(test_model(), vec![], tools, vec![], ToolContext::new());
        let names: Vec<String> = engine.tools.iter().map(|t| t.name().to_string()).collect();
        assert_eq!(names, vec!["read_file", "edit_file"]);
        // Caller-provided tools must be wired into the chat adapter.
        let adapter_names = agent_tool_names(&engine).await;
        assert!(adapter_names.contains(&"read_file".to_string()));
        assert!(adapter_names.contains(&"edit_file".to_string()));
    }

    async fn agent_tool_names(engine: &AgenticStreamEngine) -> Vec<String> {
        engine
            .build_chat_adapter()
            .tool_server_handle()
            .static_tool_defs()
            .into_iter()
            .map(|def| def.name.to_string())
            .collect()
    }

    #[tokio::test]
    async fn test_vision_model_auto_enables_read_image() {
        let mut model = test_model();
        model.vision = true;
        let engine = AgenticStreamEngine::new(model, vec![], vec![], vec![], ToolContext::new());
        assert!(
            agent_tool_names(&engine)
                .await
                .contains(&"read_image".to_string()),
            "vision model should have read_image as a system tool"
        );
    }

    #[tokio::test]
    async fn test_non_vision_model_does_not_enable_read_image() {
        let engine =
            AgenticStreamEngine::new(test_model(), vec![], vec![], vec![], ToolContext::new());
        assert!(
            !agent_tool_names(&engine)
                .await
                .contains(&"read_image".to_string()),
            "non-vision model should not have read_image"
        );
    }

    #[tokio::test]
    async fn test_non_vision_model_enables_read_image_with_fallback_model() {
        crate::utils::PLUGIN_ROOT
            .set(std::env::current_dir().unwrap())
            .ok();
        let mut config = crate::config::TenonConfig::default();
        let mut fallback = test_model();
        fallback.vision = true;
        config.tools.read_image.fallback_model = Some(fallback);
        let _config = crate::ConfigOverride::set(config);

        let engine =
            AgenticStreamEngine::new(test_model(), vec![], vec![], vec![], ToolContext::new());
        assert!(
            agent_tool_names(&engine)
                .await
                .contains(&"read_image".to_string()),
            "non-vision model with a fallback vision model should have read_image"
        );
    }

    #[tokio::test]
    async fn test_switching_model_updates_read_image_availability() {
        let mut model = test_model();
        model.vision = true;
        let mut engine =
            AgenticStreamEngine::new(model, vec![], vec![], vec![], ToolContext::new());
        assert!(
            agent_tool_names(&engine)
                .await
                .contains(&"read_image".to_string()),
            "vision model should have read_image as a system tool"
        );

        // Simulate a model switch to a non-vision model
        engine.model = test_model();
        assert!(
            !agent_tool_names(&engine)
                .await
                .contains(&"read_image".to_string()),
            "switching to a non-vision model should drop read_image"
        );

        // And back to a vision model
        engine.model.vision = true;
        assert!(
            agent_tool_names(&engine)
                .await
                .contains(&"read_image".to_string()),
            "switching back to a vision model should restore read_image"
        );
    }

    #[test]
    fn test_load_reconstructs_active_choreo_from_logs() {
        crate::utils::PLUGIN_ROOT
            .set(std::env::current_dir().unwrap())
            .ok();

        let log_handler = ChatLogHandler::new();
        let tools = resolve_tools(
            &["read_file".to_string(), "edit_file".to_string()],
            log_handler.log_window.clone(),
        );
        let mut engine =
            AgenticStreamEngine::new(test_model(), vec![], tools, vec![], ToolContext::new());

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

    /// Minimal Ollama /api/chat mock over raw TCP. The first `fail_times`
    /// connections serve a valid response head plus a truncated ndjson body
    /// (partial content reaches the chat, then the connection drops
    /// mid-body); every later connection serves the full body. Pass 0 for a
    /// stable mock that always serves the full body.
    async fn spawn_ollama_mock(
        body: &'static str,
        fail_times: usize,
    ) -> (SocketAddr, Arc<AtomicUsize>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("mock binds");
        let addr = listener.local_addr().expect("mock address");
        let connections = Arc::new(AtomicUsize::new(0));
        let counter = connections.clone();
        tokio::spawn(async move {
            while let Ok((mut socket, _)) = listener.accept().await {
                let count = counter.fetch_add(1, Ordering::SeqCst) + 1;
                tokio::spawn(async move {
                    if !drain_http_request(&mut socket).await {
                        return;
                    }
                    let head = format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: application/x-ndjson\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        body.len()
                    );
                    let _ = socket.write_all(head.as_bytes()).await;
                    if count <= fail_times {
                        // Cut mid-body after the first complete line so partial
                        // content is delivered before the failure.
                        let cut = body.find('\n').map_or(body.len(), |i| i + 1 + 10);
                        let _ = socket.write_all(&body.as_bytes()[..cut]).await;
                    } else {
                        let _ = socket.write_all(body.as_bytes()).await;
                    }
                });
            }
        });
        (addr, connections)
    }

    /// Reads one HTTP request (head + Content-Length body) off the socket.
    async fn drain_http_request(socket: &mut TcpStream) -> bool {
        let mut buf: Vec<u8> = Vec::new();
        let mut chunk = [0u8; 4096];
        let head_end = loop {
            match socket.read(&mut chunk).await {
                Ok(0) => return false,
                Ok(n) => buf.extend_from_slice(&chunk[..n]),
                Err(_) => return false,
            }
            if let Some(pos) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                break pos + 4;
            }
        };
        let content_length = String::from_utf8_lossy(&buf[..head_end])
            .lines()
            .find_map(|line| {
                let (name, value) = line.split_once(':')?;
                if name.trim().eq_ignore_ascii_case("content-length") {
                    value.trim().parse::<usize>().ok()
                } else {
                    None
                }
            })
            .unwrap_or(0);
        while buf.len() < head_end + content_length {
            match socket.read(&mut chunk).await {
                Ok(0) => return false,
                Ok(n) => buf.extend_from_slice(&chunk[..n]),
                Err(_) => return false,
            }
        }
        true
    }

    #[tokio::test]
    async fn mid_stream_failure_is_retried_with_complete_response() {
        crate::utils::PLUGIN_ROOT
            .set(std::env::current_dir().expect("test cwd"))
            .ok();
        let body = concat!(
            "{\"model\":\"test\",\"message\":{\"role\":\"assistant\",\"content\":\"Hello \"},\"done\":false}\n",
            "{\"model\":\"test\",\"message\":{\"role\":\"assistant\",\"content\":\"world\"},\"done\":false}\n",
            "{\"model\":\"test\",\"message\":{\"role\":\"assistant\",\"content\":\"\"},\"done\":true,\"done_reason\":\"stop\",\"prompt_eval_count\":1,\"eval_count\":2}\n",
        );
        let (addr, connections) = spawn_ollama_mock(body, 1).await;

        let model = SupportedModels {
            connector_name: "test".to_string(),
            config: ProviderConfig {
                kind: ProviderKind::Ollama(OllamaProviderConfig {
                    base_url: format!("http://{addr}"),
                    bearer: None,
                }),
            },
            model_name: "test".to_string(),
            default_parameters: serde_json::Map::new(),
            vision: false,
        };
        let mut engine =
            AgenticStreamEngine::new(model, vec![], vec![], vec![], ToolContext::new());

        let cancel = AtomicBool::new(false);
        engine
            .process_turn("Say hello".to_string(), &cancel, |_| {}, 1)
            .await;

        let assistant_text = engine
            .log_handler
            .log_window
            .read()
            .expect("log window readable")
            .logs
            .iter()
            .filter_map(|indexed| {
                let log = indexed.log.read().ok()?;
                let TenonLogData::Assistant(message) = log.data() else {
                    return None;
                };
                Some(
                    message
                        .content
                        .iter()
                        .map(|content| match content {
                            TenonAssistantMessageContent::Text(text) => text.as_str(),
                        })
                        .collect::<String>(),
                )
            })
            .collect::<String>();
        assert_eq!(
            assistant_text, "Hello world",
            "the partial response must be pruned before the retry delivers the complete one"
        );
        assert_eq!(
            connections.load(Ordering::SeqCst),
            2,
            "a transient mid-stream failure must be retried"
        );
    }

    #[tokio::test]
    async fn completion_call_marks_last_assistant_message_complete() {
        crate::utils::PLUGIN_ROOT
            .set(std::env::current_dir().expect("test cwd"))
            .ok();
        let body = concat!(
            "{\"model\":\"test\",\"message\":{\"role\":\"assistant\",\"content\":\"Hello \"},\"done\":false}\n",
            "{\"model\":\"test\",\"message\":{\"role\":\"assistant\",\"content\":\"world\"},\"done\":false}\n",
            "{\"model\":\"test\",\"message\":{\"role\":\"assistant\",\"content\":\"\"},\"done\":true,\"done_reason\":\"stop\",\"prompt_eval_count\":1,\"eval_count\":2}\n",
        );
        let (addr, _connections) = spawn_ollama_mock(body, 0).await;

        let model = SupportedModels {
            connector_name: "test".to_string(),
            config: ProviderConfig {
                kind: ProviderKind::Ollama(OllamaProviderConfig {
                    base_url: format!("http://{addr}"),
                    bearer: None,
                }),
            },
            model_name: "test".to_string(),
            default_parameters: serde_json::Map::new(),
            vision: false,
        };
        let mut engine =
            AgenticStreamEngine::new(model, vec![], vec![], vec![], ToolContext::new());

        let cancel = AtomicBool::new(false);
        engine
            .process_turn("Say hello".to_string(), &cancel, |_| {}, 1)
            .await;

        let log_window = engine
            .log_handler
            .log_window
            .read()
            .expect("log window readable");
        let last = log_window.logs.last().expect("assistant log exists");
        let log = last.log.read().expect("log readable");
        let TenonLogData::Assistant(message) = log.data() else {
            panic!("last log should be the assistant message");
        };
        assert!(
            message.complete,
            "CompletionCall must mark the assistant message as complete"
        );
    }

    #[tokio::test]
    async fn persistent_mid_stream_failure_is_bounded_and_surfaces_error() {
        crate::utils::PLUGIN_ROOT
            .set(std::env::current_dir().expect("test cwd"))
            .ok();
        let body = concat!(
            "{\"model\":\"test\",\"message\":{\"role\":\"assistant\",\"content\":\"Hello \"},\"done\":false}\n",
            "{\"model\":\"test\",\"message\":{\"role\":\"assistant\",\"content\":\"world\"},\"done\":false}\n",
            "{\"model\":\"test\",\"message\":{\"role\":\"assistant\",\"content\":\"\"},\"done\":true,\"done_reason\":\"stop\",\"prompt_eval_count\":1,\"eval_count\":2}\n",
        );
        // Every connection truncates mid-body, so every attempt fails.
        let (addr, connections) = spawn_ollama_mock(body, usize::MAX).await;

        let model = SupportedModels {
            connector_name: "test".to_string(),
            config: ProviderConfig {
                kind: ProviderKind::Ollama(OllamaProviderConfig {
                    base_url: format!("http://{addr}"),
                    bearer: None,
                }),
            },
            model_name: "test".to_string(),
            default_parameters: serde_json::Map::new(),
            vision: false,
        };
        let mut engine =
            AgenticStreamEngine::new(model, vec![], vec![], vec![], ToolContext::new());

        let cancel = AtomicBool::new(false);
        // The surfaced error notifies through the Neovim execution handler,
        // whose initialization panics outside Neovim. Catch the unwind so the
        // assertions below report the bounded-retry behavior instead of the
        // environment panic.
        let _ = AssertUnwindSafe(engine.process_turn("Say hello".to_string(), &cancel, |_| {}, 1))
            .catch_unwind()
            .await;

        let assistant_text = engine
            .log_handler
            .log_window
            .read()
            .expect("log window readable")
            .logs
            .iter()
            .filter_map(|indexed| {
                let log = indexed.log.read().ok()?;
                let TenonLogData::Assistant(message) = log.data() else {
                    return None;
                };
                Some(
                    message
                        .content
                        .iter()
                        .map(|content| match content {
                            TenonAssistantMessageContent::Text(text) => text.as_str(),
                        })
                        .collect::<String>(),
                )
            })
            .collect::<String>();
        assert_eq!(
            assistant_text, "Hello ",
            "each attempt's partial response is pruned, leaving only the last attempt's"
        );
        assert_eq!(
            connections.load(Ordering::SeqCst),
            3,
            "persistent mid-stream failures must stop after the attempt bound"
        );
    }

    /// Concatenates the text content of every assistant log into one string.
    fn assistant_text_of(engine: &AgenticStreamEngine) -> String {
        engine
            .log_handler
            .log_window
            .read()
            .expect("log window readable")
            .logs
            .iter()
            .filter_map(|indexed| {
                let log = indexed.log.read().ok()?;
                let TenonLogData::Assistant(message) = log.data() else {
                    return None;
                };
                Some(
                    message
                        .content
                        .iter()
                        .map(|content| match content {
                            TenonAssistantMessageContent::Text(text) => text.as_str(),
                        })
                        .collect::<String>(),
                )
            })
            .collect()
    }

    async fn engine_streaming(body: &'static str) -> AgenticStreamEngine {
        crate::utils::PLUGIN_ROOT
            .set(std::env::current_dir().expect("test cwd"))
            .ok();
        let (addr, _connections) = spawn_ollama_mock(body, 0).await;
        let model = SupportedModels {
            connector_name: "test".to_string(),
            config: ProviderConfig {
                kind: ProviderKind::Ollama(OllamaProviderConfig {
                    base_url: format!("http://{addr}"),
                    bearer: None,
                }),
            },
            model_name: "test".to_string(),
            default_parameters: serde_json::Map::new(),
            vision: false,
        };
        let mut engine =
            AgenticStreamEngine::new(model, vec![], vec![], vec![], ToolContext::new());
        let cancel = AtomicBool::new(false);
        engine
            .process_turn("Say hello".to_string(), &cancel, |_| {}, 1)
            .await;
        engine
    }

    #[tokio::test]
    async fn tenon_chat_keyword_is_filtered_from_streamed_text() {
        // The keyword is only filtered at line starts: message start and
        // after a newline. Mid-line occurrences are legitimate content and
        // pass through untouched.
        let body = concat!(
            "{\"model\":\"test\",\"message\":{\"role\":\"assistant\",\"content\":\"::tenon\"},\"done\":false}\n",
            "{\"model\":\"test\",\"message\":{\"role\":\"assistant\",\"content\":\"-chat:: \"},\"done\":false}\n",
            "{\"model\":\"test\",\"message\":{\"role\":\"assistant\",\"content\":\"Hello\"},\"done\":false}\n",
            "{\"model\":\"test\",\"message\":{\"role\":\"assistant\",\"content\":\"::tenon-chat::\"},\"done\":false}\n",
            "{\"model\":\"test\",\"message\":{\"role\":\"assistant\",\"content\":\"\\n\"},\"done\":false}\n",
            "{\"model\":\"test\",\"message\":{\"role\":\"assistant\",\"content\":\"::tenon\"},\"done\":false}\n",
            "{\"model\":\"test\",\"message\":{\"role\":\"assistant\",\"content\":\"-chat::Bye\"},\"done\":false}\n",
            "{\"model\":\"test\",\"message\":{\"role\":\"assistant\",\"content\":\"\"},\"done\":true,\"done_reason\":\"stop\",\"prompt_eval_count\":1,\"eval_count\":2}\n",
        );
        let engine = engine_streaming(body).await;
        assert_eq!(
            assistant_text_of(&engine),
            "Hello::tenon-chat::\nBye",
            "line-start ::tenon-chat:: must be dropped (with one trailing space); mid-line occurrences pass through"
        );
    }

    #[tokio::test]
    async fn keyword_prefix_that_never_completes_is_released() {
        let body = concat!(
            "{\"model\":\"test\",\"message\":{\"role\":\"assistant\",\"content\":\"::tenon\"},\"done\":false}\n",
            "{\"model\":\"test\",\"message\":{\"role\":\"assistant\",\"content\":\"-chat is fine\"},\"done\":false}\n",
            "{\"model\":\"test\",\"message\":{\"role\":\"assistant\",\"content\":\"\"},\"done\":true,\"done_reason\":\"stop\",\"prompt_eval_count\":1,\"eval_count\":2}\n",
        );
        let engine = engine_streaming(body).await;
        assert_eq!(
            assistant_text_of(&engine),
            "::tenon-chat is fine",
            "held text that turns out not to be the keyword must be released"
        );
    }

    #[tokio::test]
    async fn held_text_is_released_when_turn_ends() {
        let body = concat!(
            "{\"model\":\"test\",\"message\":{\"role\":\"assistant\",\"content\":\"::tenon\"},\"done\":false}\n",
            "{\"model\":\"test\",\"message\":{\"role\":\"assistant\",\"content\":\"\"},\"done\":true,\"done_reason\":\"stop\",\"prompt_eval_count\":1,\"eval_count\":2}\n",
        );
        let engine = engine_streaming(body).await;
        assert_eq!(
            assistant_text_of(&engine),
            "::tenon",
            "text still held when the turn ends must be resolved instead of dropped"
        );
    }
}
