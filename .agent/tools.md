# Creating a Tool

Tool = unit of capability agent invokes during conversation.

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

## 2. Define tool struct

Unit struct with `#[derive(Deserialize, Serialize, Clone)]`:

```rust
#[derive(Deserialize, Serialize, Clone)]
pub struct MyTool;
```

## 3. Implement `Tool` trait

Implement `rig::tool::Tool`. Must define:

| Assoc constant/type | Value |
|---|---|
| `NAME` | `&'static str` (e.g. `"my_tool"`) |
| `Error` | `ToolError` |
| `Args` | Args struct (e.g. `MyToolArgs`) |
| `Output` | `String` |

Required methods:

- **`description(&self) -> String`** → returns the tool description.
- **`parameters(&self) -> serde_json::Value`** → returns JSON Schema.
  Describe each property + list required fields.
- **`async call(&self, args: Self::Args) -> Result<Self::Output, Self::Error>`** →
  actual logic. Read file / perform op → return `String`. On failure →
  `Err(ToolError::ToolCallError(Box::new(...)))` with descriptive
  `std::io::Error`.

## 4. Register tool

In `src/tools/mod.rs`, add:

```rust
pub mod my_tool;
pub use my_tool::MyTool;
```

Also add entry to the `all_tools` vec inside `resolve_tools()`:

```rust
(
    "my_tool".to_string(),
    Box::new(MyTool) as Box<dyn ToolDyn>,
),
```

## 5. Tool display summary

Chat UI shows core arg per tool call: `[web_search] Running.. "rust neovim"`.

Add match arm in `src/tools/mod.rs` → `tool_display_summary()`:

```rust
"my_tool" => "filepath",   // arg that describes what's happening
```

No useful display arg → omit. Fallback: `[tool_name] Done!`.
