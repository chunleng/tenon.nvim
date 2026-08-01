use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use nvim_oxi::{
    Result as OxiResult,
    api::{self, opts::CreateAutocmdOpts},
    mlua::{LuaSerdeExt, lua},
};

use crate::utils::GLOBAL_EXECUTION_HANDLER;

enum FzfAction {
    Fn(ActionBuilder),
    Str(String),
}

#[derive(Clone)]
enum SelectMode {
    Single { current_selected: Option<String> },
    Multi { current_selected: Vec<String> },
}

struct FzfOption {
    prompt: String,
    sorting: bool,
    select_mode: SelectMode,
    actions: HashMap<String, FzfAction>,
    callback: Box<dyn FnOnce(Option<Vec<String>>) + Send>,
}

type ResolveCell = Arc<Mutex<Option<Box<dyn FnOnce(OxiResult<Vec<String>>) + Send>>>>;

type ActionBuilder =
    Box<dyn FnOnce(&mlua::Lua, mlua::Function) -> OxiResult<mlua::Function> + Send>;

fn action(
    f: impl FnOnce(&mlua::Lua, mlua::Function) -> OxiResult<mlua::Function> + Send + 'static,
) -> ActionBuilder {
    Box::new(f)
}

/// Creates a Lua `resolve` function bridging to the Rust resolve callback.
/// The resolve_cell ensures resolve is only called once (take() returns None
/// after the first call), replacing the Lua `resolved` flag.
fn create_resolve_fn(lua: &mlua::Lua, resolve_cell: &ResolveCell) -> OxiResult<mlua::Function> {
    let cell = resolve_cell.clone();
    Ok(lua.create_function(move |lua, value: mlua::Value| {
        if let Some(r) = cell.lock().ok().and_then(|mut g| g.take()) {
            let result = lua
                .from_value::<Vec<String>>(value)
                .map_err(nvim_oxi::Error::Mlua);
            r(result);
        }
        Ok(())
    })?)
}

/// Creates the `on_create` callback that registers a WinClosed autocmd.
/// When the fzf window closes without a selection, resolves with
/// `{error = "cancelled"}` after a 3-second delay (via `vim.defer_fn`)
/// to let fzf's selection action fire first.
fn create_on_create_fn(
    lua_ref: &mlua::Lua,
    resolve_cell: &ResolveCell,
) -> OxiResult<mlua::Function> {
    let cell = resolve_cell.clone();
    Ok(lua_ref.create_function(move |_, ()| {
        let winid = api::get_current_win();
        let winid_str = winid.to_string();
        let cell = cell.clone();
        let _ = api::create_autocmd(
            ["WinClosed"],
            &CreateAutocmdOpts::builder()
                .patterns([winid_str.as_str()])
                .once(true)
                .callback(move |_| {
                    let lua = lua();
                    let callback = lua.create_function({
                        let cell = cell.clone();
                        move |_, ()| {
                            if let Some(r) = cell.lock().ok().and_then(|mut g| g.take()) {
                                r(Err(nvim_oxi::Error::Mlua(mlua::Error::RuntimeError(
                                    "cancelled".to_string(),
                                ))));
                            }
                            Ok(())
                        }
                    });
                    if let Ok(callback) = callback {
                        let _ = lua.load("vim.defer_fn").call::<()>((callback, 3000));
                    }
                    false
                })
                .build(),
        );
        Ok(())
    })?)
}

/// Runs fzf-lua on the main thread asynchronously. Builds the fzf options
/// table from `FzfOption` (prompt, fzf_opts, keymap, actions, winopts).
/// Returns the selected items as a list of strings.
fn run_fzf(mut options: Vec<String>, fzf_option: FzfOption) -> OxiResult<()> {
    let select_mode = fzf_option.select_mode.clone();
    let callback = fzf_option.callback;
    let result = GLOBAL_EXECUTION_HANDLER.execute_rust_on_main_thread_async(move |resolve| {
        let lua = lua();
        let resolve_cell: ResolveCell = Arc::new(Mutex::new(Some(resolve)));

        let result: OxiResult<()> = (|| {
            let resolve_fn = create_resolve_fn(&lua, &resolve_cell)?;
            let on_create_fn = create_on_create_fn(&lua, &resolve_cell)?;
            let opts = lua.create_table()?;

            opts.set("prompt", format!("{}> ", fzf_option.prompt))?;

            let fzf_opts = lua.create_table()?;
            if !fzf_option.sorting {
                fzf_opts.set("--no-sort", "")?;
            }

            let actions = lua.create_table()?;
            let default_resolve = resolve_fn.clone();
            let default_fn = lua.create_function(move |_, sel: Vec<String>| {
                default_resolve.call::<()>(sel)?;
                Ok(())
            })?;
            actions.set("default", default_fn)?;

            let mut fzf_keymap = None::<mlua::Table>;

            match fzf_option.select_mode {
                SelectMode::Multi { current_selected } => {
                    fzf_opts.set("--multi", "")?;
                    fzf_opts.set("--marker", "✓")?;
                    fzf_opts.set("--header", "(use <TAB> to toggle  <ENTER> to confirm)")?;

                    // Sort: current selections first, then the rest.
                    let mut sorted: Vec<String> = options
                        .iter()
                        .filter(|o| current_selected.contains(o))
                        .cloned()
                        .collect();
                    sorted.extend(
                        options
                            .iter()
                            .filter(|o| !current_selected.contains(o))
                            .cloned(),
                    );
                    options = sorted;

                    let keymap = lua.create_table()?;
                    keymap.set("tab", "toggle+down")?;
                    if !current_selected.is_empty() {
                        let load_action = "select+down+".repeat(current_selected.len());
                        keymap.set("load", load_action.trim_end_matches('+'))?;
                    }
                    fzf_keymap = Some(keymap);

                    let ctrl_x_resolve = resolve_fn.clone();
                    let ctrl_x_fn = lua.create_function(move |_, ()| {
                        ctrl_x_resolve.call::<()>(Vec::<String>::new())?;
                        Ok(())
                    })?;
                    actions.set("ctrl-x", ctrl_x_fn)?;
                }
                SelectMode::Single { current_selected } => {
                    fzf_opts.set("--no-multi", "")?;
                    options = options
                        .iter()
                        .map(|opt| {
                            if current_selected.as_deref() == Some(opt.as_str()) {
                                format!("{}{}", CURRENT_MARKER, opt)
                            } else {
                                format!("{}{}", OTHER_MARKER, opt)
                            }
                        })
                        .collect();
                }
            }
            opts.set("fzf_opts", fzf_opts)?;

            for (key, action) in fzf_option.actions {
                match action {
                    FzfAction::Fn(builder) => {
                        let action_fn = builder(&lua, resolve_fn.clone())?;
                        actions.set(key, action_fn)?;
                    }
                    FzfAction::Str(s) => {
                        let keymap = match &fzf_keymap {
                            Some(t) => t,
                            None => {
                                let t = lua.create_table()?;
                                fzf_keymap = Some(t);
                                fzf_keymap.as_ref().unwrap()
                            }
                        };
                        keymap.set(key, s)?;
                    }
                }
            }
            opts.set("actions", actions)?;
            if let Some(fzf_keymap) = fzf_keymap {
                let keymap = lua.create_table()?;
                keymap.set("fzf", fzf_keymap)?;
                opts.set("keymap", keymap)?;
            }

            let winopts = lua.create_table()?;
            winopts.set("on_create", on_create_fn)?;
            opts.set("winopts", winopts)?;

            let options_table = lua.create_table()?;
            for (i, opt) in options.into_iter().enumerate() {
                options_table.set(i + 1, opt)?;
            }

            let fzf_exec = lua
                .load("return require('fzf-lua').fzf_exec")
                .eval::<mlua::Function>()?;
            fzf_exec.call::<()>((options_table, opts))?;
            Ok(())
        })();

        if let Err(e) = result {
            if let Some(r) = resolve_cell.lock().ok().and_then(|mut g| g.take()) {
                r(Err(e));
            }
        }
    });

    let result = match select_mode {
        SelectMode::Single { .. } => {
            result.map(|items| items.into_iter().map(|s| clean_marker(&s)).collect())
        }
        SelectMode::Multi { .. } => result,
    };

    callback(result.ok());

    Ok(())
}

const CURRENT_MARKER: &str = "> ";
const OTHER_MARKER: &str = "  ";

/// Shows a FzfLua single-select picker.
///
/// Displays `prompt` as the header and `options` as selectable items.
/// If `current` is `Some(item)`, that item is visually marked with `>`.
/// The result is delivered asynchronously via `on_select` (called with
/// `Some(selection)` or `None` if cancelled).
///
/// This function is non-blocking and safe to call from the main thread
/// (e.g. from a keymap handler).
pub fn pick(
    prompt: &str,
    options: &[&str],
    current: Option<&str>,
    on_select: impl FnOnce(Option<String>) + Send + 'static,
) -> OxiResult<()> {
    let options: Vec<String> = options.iter().map(|s| s.to_string()).collect();
    let current_selected = current.map(|s| s.to_string());

    let prompt_clone = prompt.to_string();
    std::thread::spawn(move || {
        let _ = run_fzf(
            options,
            FzfOption {
                prompt: prompt_clone,
                sorting: false,
                select_mode: SelectMode::Single { current_selected },
                actions: HashMap::new(),
                callback: Box::new(move |items| {
                    on_select(items.and_then(|v| v.into_iter().next()));
                }),
            },
        );
    });

    Ok(())
}

fn clean_marker(s: &str) -> String {
    s.strip_prefix(CURRENT_MARKER)
        .or_else(|| s.strip_prefix(OTHER_MARKER))
        .unwrap_or(s)
        .to_string()
}

/// Shows a FzfLua multi-select picker.
///
/// Displays `prompt` as the header, `options` as selectable items. Items in
/// `current` are sorted to the top and pre-selected via `select+down` on the
/// fzf `load` event, so fzf's native `✓` marker appears on them. Users press
/// TAB to toggle selection (and advance to the next item), then ENTER to
/// confirm. The result is delivered asynchronously via `on_select`:
/// - `Some(vec)` — user confirmed with selected items
/// - `None` — user cancelled or an error occurred (do not change tools)
///
/// This function is non-blocking and safe to call from the main thread.
pub fn pick_multi(
    prompt: &str,
    options: &[&str],
    current: &[&str],
    on_select: impl FnOnce(Option<Vec<String>>) + Send + 'static,
) -> OxiResult<()> {
    let current_selected: Vec<String> = current.iter().map(|s| s.to_string()).collect();
    let raw_options: Vec<String> = options.iter().map(|s| s.to_string()).collect();

    let prompt_clone = prompt.to_string();
    std::thread::spawn(move || {
        let _ = run_fzf(
            raw_options,
            FzfOption {
                prompt: prompt_clone,
                sorting: false,
                select_mode: SelectMode::Multi { current_selected },
                actions: HashMap::new(),
                callback: Box::new(on_select),
            },
        );
    });

    Ok(())
}
