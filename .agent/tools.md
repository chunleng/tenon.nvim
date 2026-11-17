# Creating a Tool

Tool = unit of capability agent invokes during conversation.

All tools implement `ToolCore` and `ToolCoreCall` from `crate::tools`, then register through `TenonTool`. Do not implement Rig's tool trait directly or register a bare tool. `TenonTool::new(tool, log_window).into()` produces a `rig::tool::DynamicTool` and handles argument deserialization, chat logs, progress, results, and errors.

## 1. Define args struct

Struct with `#[derive(Deserialize)]` **and** `#[serde(deny_unknown_fields)]`. Holds tool parameters.
Optional fields → `Option<T>`.

```rust
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MyToolArgs {
    pub filepath: String,
    pub some_option: Option<usize>,
}
```

`deny_unknown_fields` causes deserialization to fail on unexpected parameters instead of silently ignoring them. Always add it to Args structs so invalid tool calls surface as errors.

## 2. Define tool and per-call structs

The tool holds shared configuration. The call holds arguments and state for one invocation.

```rust
pub struct MyTool;

pub struct MyToolCall {
    args: MyToolArgs,
}
```

## 3. Implement `ToolCore` and `ToolCoreCall`

`ToolCore` defines the tool contract:

| Associated type | Value |
|---|---|
| `Args` | Args struct, implementing `DeserializeOwned + Send + Sync` |
| `Output` | Type implementing `IntoToolOutput`, usually `String` |
| `Error` | Error implementing `std::error::Error + Send + Sync + 'static`, usually `ToolExecutionError` |
| `Call` | Per-call struct implementing `ToolCoreCall` with matching `Output` and `Error` |

Required methods:

- **`name(&self) -> String`** → non-empty, unique registration and provider-facing name.
- **`description(&self) -> String`** → model-facing description.
- **`parameters(&self) -> serde_json::Value`** → JSON Schema. Describe each property and list required fields; keep it consistent with the args struct.
- **`properties(&self, args: &Self::Args) -> LogProperties`** → classification for RAG and context truncation. Set `idempotent: true` for operations with no side effects that are safe to rerun. Set `important: true` for Tenon system logs such as choreo management and work queue operations. When unsure how to set either flag, ask the user instead of assuming.
- **`async init_call(&self, context: &mut ToolContext, args: Self::Args) -> Result<Self::Call, Self::Error>`** → validate arguments, initialize per-call state, and optionally start work.

`ToolCoreCall` defines execution. Its required `async result(self, context: &mut ToolContext)` consumes the call and returns `Result<Self::Output, Self::Error>`.

Example skeleton (replace the placeholder result with actual logic):

```rust
use crate::chat::log::LogProperties;
use crate::tools::{ToolCore, ToolCoreCall};
use rig::tool::{ToolContext, ToolExecutionError};

impl ToolCore for MyTool {
    type Args = MyToolArgs;
    type Output = String;
    type Error = ToolExecutionError;
    type Call = MyToolCall;

    fn name(&self) -> String {
        "my_tool".to_string()
    }

    fn description(&self) -> String {
        "Describe what the tool does and when to use it".to_string()
    }

    fn parameters(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "filepath": { "type": "string", "description": "Path to file" },
                "some_option": { "type": "integer", "description": "Optional setting" }
            },
            "required": ["filepath"]
        })
    }

    fn properties(&self, _args: &Self::Args) -> LogProperties {
        LogProperties { idempotent: true, important: false }
    }

    async fn init_call(
        &self,
        _context: &mut ToolContext,
        args: Self::Args,
    ) -> Result<Self::Call, Self::Error> {
        Ok(MyToolCall { args })
    }
}

impl ToolCoreCall for MyToolCall {
    type Output = String;
    type Error = ToolExecutionError;

    async fn result(self, _context: &mut ToolContext) -> Result<Self::Output, Self::Error> {
        Ok(format!("Requested path: {}", self.args.filepath))
    }
}
```

Return failures on the error channel, for example `Err(ToolExecutionError::invalid_args("descriptive validation error"))` or `Err(ToolExecutionError::other(format!("operation failed: {err}")))`. The wrapper records them in the chat log.

Optional hooks:

- **`ToolCoreCall::stream(&mut self, context: &mut ToolContext) -> Option<BoxStream<'_, String>>`** → progress strings for live visibility. Defaults to `None`. The wrapper drains the stream before calling `result()`.
- **`ToolCore::convert_log(&self, log: Arc<RwLock<TenonLog>>) -> Option<ToolOutput>`** (async) → convert the log after a successful result has been written. Return `None` to keep the original model output, or `Some(output)` to replace it. Errors do not reach this hook. See `ask_question` and the choreo tools.

Use `ToolContext` for runtime dependencies rather than capturing per-conversation state in a unit tool. See `ask_question` for scoped event-channel access and `read_file` for approval handling.

## 4. Register tool

For a selectable built-in tool, update `src/tools/mod.rs`:

1. Declare and re-export the module:

   ```rust
   pub mod my_tool;
   pub use my_tool::MyTool;
   ```

2. Add its name to `all_tool_names()` so selection and name resolution include it:

   ```rust
   "my_tool".into(),
   ```

3. Add an entry to the `all_tools` vec in `builtin_tools(log_window)`:

   ```rust
   Some((
       "my_tool".to_string(),
       TenonTool::new(MyTool, log_window.clone()).into(),
   )),
   ```

Keep the entry's name identical to `ToolCore::name()`. For configured tools, use the same availability condition in `all_tool_names()` and `builtin_tools()`. `resolve_tools(names, log_window)` selects from these entries and also wraps MCP tools with `TenonTool`.

## 5. Tool display summary

Chat UI shows a summary of the core argument for each tool call.

Add a match arm in `src/tools/mod.rs` → `tool_display_summary()`:

```rust
"my_tool" => "filepath",   // arg that describes what's happening
```

No useful display arg → omit; `tool_display_summary()` returns `None`.

If the meaningful info lives in the tool output instead of args, add a special case (see `pop_task` in `tool_display_summary()`).
