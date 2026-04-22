# Tenon

Neovim plugin. Pure Rust. Uses `nvim-oxi` (safe, idiomatic Rust bindings to
Neovim API). Agentic chat tool → responds to requests + calls tools.

## Dev Guide

### Build & Format

After changing Rust code:

1. `cargo build` → verify no breakage
2. `cargo fmt` → format code

### Main Thread Guide

Neovim = single-threaded. All Lua/API calls **must** run on main thread.

Off-thread code (async tasks, Tokio runtime, `Tool::call()`, etc.) → **never**
call Neovim APIs directly.

#### `GLOBAL_EXECUTION_HANDLER`

Bridge: off-thread → main-thread. Lives in `src/utils.rs`.

```rust
use crate::utils::GLOBAL_EXECUTION_HANDLER;

// From any thread:
let result: serde_json::Value = GLOBAL_EXECUTION_HANDLER
    .execute_on_main_thread("vim.api.nvim_get_current_line()")?;
```

**How it works:**

1. Caller sends `(lua_code, response_tx)` via `mpsc::channel`
2. `AsyncHandle` wakes Neovim event loop → callback runs on main thread
3. Callback: `lua().load(code).eval()` → result serialized → sent back via
channel
4. Caller blocks on `rx.recv()` → gets `serde_json::Value`

**Guide:**

- Need Neovim API from off-thread? → use
  `GLOBAL_EXECUTION_HANDLER.execute_on_main_thread()`
- Need async Lua from off-thread (callbacks, deferred work)? → use
  `GLOBAL_EXECUTION_HANDLER.execute_on_main_thread_async()`.
  Lua code receives a `resolve` callback — call `resolve(value)` to return result.
- On main thread already? → call API directly, no handler needed
- `LazyLock` → single global instance, lazy init

## UI Architecture

```
View          ← layout of Panels, user "page"
 └ Panel      ← owns NvimBuffer + NvimWindow, hosts Widgets
     └ Widget ← renders into NvimBuffer (no window)
         └ NvimBuffer / NvimWindow ← raw API wrappers (nvim_primitives)
```

| Layer | Role | Current |
|-------|------|---------|
| **View** | Top-level page. Owns Panel layout, wires Widgets. | `ui/mod.rs` |
| **Panel** | Owns buffer + window. Split/float/tile surface. Hosts Widgets. | `ui/panels/` |
| **Widget** | Embeddable control. Renders into buffer, no window. | `ui/widget/` |
| **nvim_primitives** | Thin `nvim_buf_*` / `nvim_win_*` wrappers. | `ui/nvim_primitives/` |

### Target structure

```
src/ui/
  mod.rs
  panels/
    mod.rs
    fixed.rs
    swappable.rs
  widget/
    mod.rs
    display.rs
  nvim_primitives/
    mod.rs
    buffer.rs
    window.rs
```

## Dev Workflow

### Creating a Tool

Tool = unit of capability agent invokes during conversation.
Steps:

#### 1. Define args struct

Struct with `#[derive(Deserialize)]`. Holds tool parameters.
Optional fields → `Option<T>`.

```rust
#[derive(Deserialize)]
pub struct MyToolArgs {
    pub filepath: String,
    pub some_option: Option<usize>,
}
```

#### 2. Define tool struct

Unit struct with `#[derive(Deserialize, Serialize, Clone)]`:

```rust
#[derive(Deserialize, Serialize, Clone)]
pub struct MyTool;
```

#### 3. Implement `Tool` trait

Implement `rig::tool::Tool`. Must define:

| Assoc constant/type | Value |
|---|---|
| `NAME` | `&'static str` (e.g. `"my_tool"`) |
| `Error` | `ToolError` |
| `Args` | Args struct (e.g. `MyToolArgs`) |
| `Output` | `String` |

Required async methods:

- **`definition(&self, _prompt: String) -> ToolDefinition`** → returns JSON
  Schema (`name`, `description`, `parameters`). Describe each property + list
  required fields.
- **`call(&self, args: Self::Args) -> Result<Self::Output, Self::Error>`** →
  actual logic. Read file / perform op → return `String`. On failure →
  `Err(ToolError::ToolCallError(Box::new(...)))` with descriptive
  `std::io::Error`.

#### 4. Register tool

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
