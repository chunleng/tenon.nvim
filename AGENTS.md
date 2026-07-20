# Tenon

Neovim plugin. Pure Rust. `nvim-oxi` bindings. Agentic chat tool.

## Build & Format
1. `cargo build`
2. `cargo test <module>`
3. `cargo fmt`
4. `cargo clippy`

## Main Thread Guide

Neovim = single-threaded. All Lua/API calls **must** run on main thread.

Off-thread code → **never** call Neovim APIs directly.

### GLOBAL_EXECUTION_HANDLER

Bridge: off-thread → main-thread. Lives in `src/utils.rs`.

**Three methods:**

1. `execute_on_main_thread(lua_code)` → sync Lua
2. `execute_on_main_thread_async(lua_code)` → async Lua, receives `resolve`
3. `execute_rust_on_main_thread(closure)` → Rust closure (type-safe, preferred)

**Usage:**

```rust
// Lua API from off-thread
let line: Value = GLOBAL_EXECUTION_HANDLER
    .execute_on_main_thread("vim.api.nvim_get_current_line()")?;

// Rust API from off-thread (preferred)
let line: String = GLOBAL_EXECUTION_HANDLER.execute_rust_on_main_thread(|| {
    api::get_current_line()
})?;
```

**Guide:**

- Off-thread → use GLOBAL_EXECUTION_HANDLER
- Main thread → call API directly
- Prefer `execute_rust_on_main_thread()` for type safety

## Common AHAs

### Buffer line edit: strings must not contain newlines

`set_lines()` treats each `String` element as a single buffer line. A `\n`
inside an element causes the edit to fail. Always split text first:

```rust
// Bad: text may contain a newline → edit fails
buffer.set_lines(start..end, false, vec![text]);

// Good: split into individual lines first
let lines: Vec<String> = text.lines().map(|s| s.to_string()).collect();
buffer.set_lines(start..end, false, lines);
```

## Deep-Dive Docs

See `.agent/` folder:
- [ui.md](.agent/ui.md) - UI architecture
- [tools.md](.agent/tools.md) - Creating tools
- [workflow.md](.agent/workflow.md) - Workflow-related information
