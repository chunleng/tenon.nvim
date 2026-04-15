# Tenon

Tenon is a neovim plugin written in purely Rust, using `nvim-oxi`, a safe,
idiomatic Rust bindings to the Neovim text editor's API. The plugin is an
Agentic chat tool that can respond to request and call relevant tools to do so.

## Development Guideline

After changing rust code, remember to ensure to check the following:

1. `cargo build` to make sure things are not breaking
2. `cargo fmt` to make sure code are formatted properly

## Development Workflow

### Creating a Tool

A tool is a unit of capability that the agent can invoke during a conversation.
To add a new tool, follow these steps:

#### 1. Define the args struct

Create a struct with `#[derive(Deserialize)]` that holds the parameters the tool
accepts. Fields that are optional should use `Option<T>`.

```rust
#[derive(Deserialize)]
pub struct MyToolArgs {
    pub filepath: String,
    pub some_option: Option<usize>,
}
```

#### 2. Define the tool struct

Create a unit struct with `#[derive(Deserialize, Serialize, Clone)]`:

```rust
#[derive(Deserialize, Serialize, Clone)]
pub struct MyTool;
```

#### 3. Implement the `Tool` trait

Implement `rig::tool::Tool` for your struct. You must define:

| Associated constant/type | Value |
|---|---|
| `NAME` | A `&'static str` identifier (e.g. `"my_tool"`) |
| `Error` | `ToolError` |
| `Args` | Your args struct (e.g. `MyToolArgs`) |
| `Output` | `String` |

Then implement the two required async methods:

- **`definition(&self, _prompt: String) -> ToolDefinition`** — Returns the
  tool's JSON Schema, including `name`, `description`, and `parameters`. The
  `parameters` object should describe each property and list required fields.
- **`call(&self, args: Self::Args) -> Result<Self::Output, Self::Error>`** — The
  actual logic. Read the file or perform the operation, then return the result
  as a String. On failure, return `Err(ToolError::ToolCallError(Box::new(...)))`
  with a descriptive `std::io::Error`.

#### 4. Register the tool

In `src/tools/mod.rs`, add two lines:

```rust
pub mod my_tool;
pub use my_tool::MyTool;
```
