# Tenon

Agentic chat tool, written with `nvim-oxi` bindings.

## Build & Format
1. `cargo build`
2. Run tests from the repository root:
   - Rust unit tests: `cargo test --lib`
   - Neovim integration tests (`tests/nvim/`): `cargo test --features nvim-tests --test nvim`
   - Run `bash scripts/smoke-test.sh` only when changing `lua/tenon/init.lua` or how `tenon.core` is exported or packaged, or when debugging a load or setup problem.
3. `cargo fmt`
4. `cargo clippy`

## Test Placement

Choose where to write each test by what it asserts, not by where the feature runs.

- **Rust unit tests:** Use them for behavior that can be verified without Neovim, even if the test uses temporary files or exercises several Rust modules.
- **Neovim integration tests (`tests/nvim/`):** Use them only when the assertion requires a real Neovim instance, such as verifying an editor notification or a buffer update.
- **Smoke test (`scripts/smoke-test.sh`):** Do not add new test cases.

When a change mixes Rust logic and Neovim wiring, cover the logic with unit tests and add one focused integration test only if existing tests do not cover the wiring.

## Main Thread Guide

Neovim is single-threaded. All Lua/API calls **must** run on main thread.

Off-thread code should **never** call Neovim APIs directly.

### GLOBAL_EXECUTION_HANDLER

Bridges off-thread to main-thread.

1. `execute_rust_on_main_thread(closure)` → Rust closure (sync)
2. `execute_rust_on_main_thread_async(closure)` → Rust closure with `Resolver<T>` (async)

The async variant passes a `Resolver<T>` to the closure. `Resolver<T>` is `Clone`and resolves only once - subsequent calls to `resolve()` are no-ops.

**Usage:**

```rust
use crate::utils::GLOBAL_EXECUTION_HANDLER;

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

`set_lines()` treats each `String` element as a single buffer line. A `\n` inside an element causes the edit to fail. Always split text first:

```rust
// Good: split into individual lines first
let lines: Vec<String> = text.lines().map(|s| s.to_string()).collect();
buffer.set_lines(start..end, false, lines);
```

## Deep-Dive Docs

See `.agent/` folder:
- [ui.md](.agent/ui.md) - UI architecture
- [tools.md](.agent/tools.md) - Creating tools
- [choreo.md](.agent/choreo.md) - Choreo-related information
