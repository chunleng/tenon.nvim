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

1. `execute_rust_on_main_thread(closure)` → Rust closure (sync)
2. `execute_rust_on_main_thread_async(closure)` → Rust closure with `Resolver<T>` (async)

The async variant passes a `Resolver<T>` to the closure. `Resolver<T>` is `Clone`
and resolves only once - subsequent calls to `resolve()` are no-ops.

**Usage:**

```rust
// Sync: returns result directly
let line: String = GLOBAL_EXECUTION_HANDLER.execute_rust_on_main_thread(|| {
    api::get_current_line()
})?;

// Async: callbacks that resolve later
let result: OxiResult<String> = GLOBAL_EXECUTION_HANDLER
    .execute_rust_on_main_thread_async(|resolver: Resolver<String>| {
        let lua = lua();
        let resolve_fn = lua.create_function(move |_, val: String| {
            resolver.resolve(Ok(val));
            Ok(())
        })?;
        let _ = lua.load("vim.defer_fn").call::<()>((resolve_fn, 0));
        Ok(())
    })?;
```

**Guide:**

- Sync operations → `execute_rust_on_main_thread()`
- Callbacks that resolve later (e.g. `vim.ui.input`) → `execute_rust_on_main_thread_async()` with `Resolver<T>`

## Common AHAs

### Buffer line edit: strings must not contain newlines

`set_lines()` treats each `String` element as a single buffer line. A `\n`
inside an element causes the edit to fail. Always split text first:

```rust
// Good: split into individual lines first
let lines: Vec<String> = text.lines().map(|s| s.to_string()).collect();
buffer.set_lines(start..end, false, lines);
```

## Deep-Dive Docs

See `.agent/` folder:
- [ui.md](.agent/ui.md) - UI architecture
- [tools.md](.agent/tools.md) - Creating tools
- [workflow.md](.agent/workflow.md) - Workflow-related information
