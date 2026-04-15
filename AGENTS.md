# Tenon

Neovim plugin. Pure Rust. Uses `nvim-oxi` (safe, idiomatic Rust bindings to
Neovim API). Agentic chat tool → responds to requests + calls tools.

## Dev Guideline

After changing Rust code:

1. `cargo build` → verify no breakage
2. `cargo fmt` → format code

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
