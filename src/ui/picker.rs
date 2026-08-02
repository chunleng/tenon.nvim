use std::collections::HashMap;

use nvim_oxi::{
    Result as OxiResult,
    mlua::{LuaSerdeExt, lua},
};

use crate::utils::{GLOBAL_EXECUTION_HANDLER, Resolver};

pub enum FzfBuiltin {
    ToggleDown,
    SelectDown,
}

impl FzfBuiltin {
    fn action_str(&self) -> &'static str {
        match self {
            FzfBuiltin::ToggleDown => "toggle+down",
            FzfBuiltin::SelectDown => "select+down",
        }
    }

    fn description(&self) -> &'static str {
        match self {
            FzfBuiltin::ToggleDown => "toggle",
            FzfBuiltin::SelectDown => "select",
        }
    }
}

pub enum FzfAction {
    Fn {
        builder: ActionBuilder,
        reload: bool,
        description: String,
    },
    FzfFn {
        fzf_fn: FzfBuiltin,
        repeat: usize,
    },
}

impl FzfAction {
    fn description(&self) -> String {
        match self {
            FzfAction::Fn { description, .. } => description.clone(),
            FzfAction::FzfFn { fzf_fn, .. } => fzf_fn.description().to_string(),
        }
    }
}

#[derive(Clone)]
pub enum SelectMode {
    Single { current_selected: Option<String> },
    Multi { current_selected: Vec<String> },
}

impl SelectMode {
    pub fn single(current: Option<impl ToString>) -> Self {
        SelectMode::Single {
            current_selected: current.map(|c| c.to_string()),
        }
    }

    pub fn multi(current: impl IntoIterator<Item = impl ToString>) -> Self {
        SelectMode::Multi {
            current_selected: current.into_iter().map(|s| s.to_string()).collect(),
        }
    }
}

pub struct FzfOption {
    pub prompt: String,
    pub sorting: bool,
    pub select_mode: SelectMode,
    pub marker: String,
    pub actions: HashMap<String, FzfAction>,
    pub callback: Box<dyn FnOnce(Option<Vec<String>>) + Send>,
}

impl Default for FzfOption {
    fn default() -> Self {
        Self {
            prompt: String::new(),
            sorting: true,
            select_mode: SelectMode::Single {
                current_selected: None,
            },
            marker: " ".to_string(),
            actions: HashMap::new(),
            callback: Box::new(|_| {}),
        }
    }
}

pub type ActionBuilder =
    Box<dyn FnOnce(&mlua::Lua, mlua::Function) -> OxiResult<mlua::Function> + Send>;

pub fn action(
    f: impl FnOnce(&mlua::Lua, mlua::Function) -> OxiResult<mlua::Function> + Send + 'static,
) -> ActionBuilder {
    Box::new(f)
}

/// Creates a Lua `resolve` function bridging to the Rust resolver.
/// The resolver ensures resolve is only called once.
fn create_resolve_fn(
    lua: &mlua::Lua,
    resolver: &Resolver<Vec<String>>,
) -> OxiResult<mlua::Function> {
    let resolver = resolver.clone();
    Ok(lua.create_function(move |lua, value: mlua::Value| {
        let result = lua
            .from_value::<Vec<String>>(value)
            .map_err(nvim_oxi::Error::Mlua);
        resolver.resolve(result);
        Ok(())
    })?)
}

/// Runs fzf-lua on the main thread asynchronously. Builds the fzf options
/// table from `FzfOption` (prompt, fzf_opts, keymap, actions, winopts).
/// Returns the selected items as a list of strings.
fn run_fzf(mut options: Vec<String>, fzf_option: FzfOption) -> OxiResult<()> {
    let callback = fzf_option.callback;
    let result = GLOBAL_EXECUTION_HANDLER.execute_rust_on_main_thread_async(move |resolver| {
        let lua = lua();

        let result: OxiResult<()> = (|| {
            let resolve_fn = create_resolve_fn(&lua, &resolver)?;
            let opts = lua.create_table()?;

            opts.set("prompt", format!("{}> ", fzf_option.prompt))?;

            let fzf_opts = lua.create_table()?;
            if !fzf_option.sorting {
                fzf_opts.set("--no-sort", "")?;
            }

            let actions = lua.create_table()?;

            let mut fzf_keymap = None::<mlua::Table>;
            let mut default_actions: HashMap<String, FzfAction> = HashMap::new();
            default_actions.insert(
                "default".to_string(),
                FzfAction::Fn {
                    builder: action(|lua, resolve_fn| {
                        Ok(lua.create_function(move |_, sel: Vec<String>| {
                            resolve_fn.call::<()>(sel)?;
                            Ok(())
                        })?)
                    }),
                    reload: false,
                    description: "confirm".to_string(),
                },
            );

            // Display field 2 (marker+value), search field 1 (clean value).
            // fzf returns the full line; extract_value recovers the clean value.
            fzf_opts.set("--delimiter", DELIMITER.to_string())?;
            fzf_opts.set("--with-nth", "2")?;
            fzf_opts.set("--nth", "1")?;

            // Description shown on the header (e.g. current value when excluded).
            let mut header_description: Option<String> = None;

            match fzf_option.select_mode {
                SelectMode::Multi { current_selected } => {
                    fzf_opts.set("--multi", "")?;
                    fzf_opts.set("--marker", fzf_option.marker)?;

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
                    options = sorted
                        .into_iter()
                        .map(|opt| format!("{}{}{}", opt, DELIMITER, opt))
                        .collect();

                    default_actions.insert(
                        "tab".to_string(),
                        FzfAction::FzfFn {
                            fzf_fn: FzfBuiltin::ToggleDown,
                            repeat: 1,
                        },
                    );
                    if !current_selected.is_empty() {
                        default_actions.insert(
                            "load".to_string(),
                            FzfAction::FzfFn {
                                fzf_fn: FzfBuiltin::SelectDown,
                                repeat: current_selected.len(),
                            },
                        );
                    }
                    default_actions.insert(
                        "ctrl-x".to_string(),
                        FzfAction::Fn {
                            builder: action(|lua, resolve_fn| {
                                Ok(lua.create_function(move |_, ()| {
                                    resolve_fn.call::<()>(Vec::<String>::new())?;
                                    Ok(())
                                })?)
                            }),
                            reload: false,
                            description: "clear all".to_string(),
                        },
                    );
                }
                SelectMode::Single { current_selected } => {
                    fzf_opts.set("--no-multi", "")?;
                    if let Some(current) = current_selected {
                        // Current is shown on the header, not as a choice.
                        options.retain(|opt| current != opt.as_str());
                        header_description = Some(format!("{}{}", fzf_option.marker, current));
                    }
                    options = options
                        .into_iter()
                        .map(|opt| format!("{}{}{}", opt, DELIMITER, opt))
                        .collect();
                }
            }
            // User-provided actions override select_mode defaults.
            default_actions.extend(fzf_option.actions);

            // Autogenerate --header from action keymap descriptions.
            let keymap_header: String = {
                let mut entries: Vec<(String, String)> = default_actions
                    .iter()
                    .filter(|(key, _)| key.as_str() != "default" && key.as_str() != "load")
                    .map(|(key, action)| (key.clone(), action.description()))
                    .collect();
                entries.sort_by(|a, b| a.0.cmp(&b.0));
                entries
                    .iter()
                    .map(|(key, desc)| format!("`{}`: {}", key, desc))
                    .collect::<Vec<_>>()
                    .join(", ")
            };
            let header = format_header(header_description.as_deref(), &keymap_header);
            if !header.is_empty() {
                fzf_opts.set("--header", header)?;
            }

            opts.set("fzf_opts", fzf_opts)?;

            for (key, action) in default_actions {
                match action {
                    FzfAction::Fn {
                        builder,
                        reload,
                        description: _,
                    } => {
                        let action_fn = builder(&lua, resolve_fn.clone())?;
                        if reload {
                            let action_table = lua.create_table()?;
                            action_table.set("fn", action_fn)?;
                            action_table.set("reload", true)?;
                            actions.set(key, action_table)?;
                        } else {
                            actions.set(key, action_fn)?;
                        }
                    }
                    FzfAction::FzfFn { fzf_fn, repeat } => {
                        let keymap = match &fzf_keymap {
                            Some(t) => t,
                            None => {
                                let t = lua.create_table()?;
                                fzf_keymap = Some(t);
                                fzf_keymap.as_ref().unwrap()
                            }
                        };
                        let base = fzf_fn.action_str();
                        let keymap_str = format!("{}+", base)
                            .repeat(repeat)
                            .trim_end_matches('+')
                            .to_string();
                        keymap.set(key, keymap_str)?;
                    }
                }
            }
            opts.set("actions", actions)?;
            if let Some(fzf_keymap) = fzf_keymap {
                let keymap = lua.create_table()?;
                keymap.set("fzf", fzf_keymap)?;
                opts.set("keymap", keymap)?;
            }

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
            resolver.resolve(Err(e));
        }
    });

    let result = result.map(|items| items.into_iter().map(|s| extract_value(&s)).collect());

    callback(result.ok());

    Ok(())
}

const DELIMITER: char = '';

/// Shows a FzfLua picker. Spawns a thread and runs fzf-lua with the given picker. Spawns a thread and runs fzf-lua with the given
/// `options` and `fzf_option` configuration. The result is delivered
/// asynchronously via `fzf_option.callback`.
///
/// This function is non-blocking and safe to call from the main thread
/// (e.g. from a keymap handler).
pub fn pick(options: &[&str], fzf_option: FzfOption) -> OxiResult<()> {
    let options: Vec<String> = options.iter().map(|s| s.to_string()).collect();
    std::thread::spawn(move || {
        let _ = run_fzf(options, fzf_option);
    });
    Ok(())
}

fn extract_value(s: &str) -> String {
    s.split(DELIMITER).next().unwrap_or(s).to_string()
}

/// Combines a description (e.g. current value) and keymap header into a single
/// header string: "description | keymap" when both present.
fn format_header(description: Option<&str>, keymap: &str) -> String {
    match (description, keymap.is_empty()) {
        (Some(desc), false) => format!("{} | {}", desc, keymap),
        (Some(desc), true) => desc.to_string(),
        (None, false) => keymap.to_string(),
        (None, true) => String::new(),
    }
}

/// Wraps a single-select callback (`Option<String>`) into the
/// `Option<Vec<String>>` shape that `FzfOption.callback` expects.
pub fn box_single_select(
    f: impl FnOnce(Option<String>) + Send + 'static,
) -> Box<dyn FnOnce(Option<Vec<String>>) + Send> {
    Box::new(move |items| f(items.and_then(|v| v.into_iter().next())))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_value() {
        // fzf returns full line "value{marker}value"; extract clean value
        assert_eq!(extract_value("apple> apple"), "apple");
        assert_eq!(extract_value("apple  apple"), "apple");
        // Values containing marker prefixes are preserved (was broken by clean_marker)
        assert_eq!(extract_value("> something> > something"), "> something");
        assert_eq!(extract_value("  spaced    spaced"), "  spaced");
        // No tab delimiter: return as-is
        assert_eq!(extract_value("plain"), "plain");
    }

    #[test]
    fn test_box_single_select() {
        let capture = || std::sync::Arc::new(std::sync::Mutex::new(None));
        let run = |input: Option<Vec<String>>, expected: Option<&str>| {
            let result = capture();
            let r = result.clone();
            box_single_select(move |s| {
                *r.lock().unwrap() = s;
            })(input);
            assert_eq!(*result.lock().unwrap(), expected.map(|s| s.to_string()));
        };

        run(Some(vec!["apple".to_string()]), Some("apple"));
        run(
            Some(vec!["first".to_string(), "second".to_string()]),
            Some("first"),
        );
        run(Some(vec![]), None);
        run(None, None);
    }

    #[test]
    fn test_format_header_both() {
        // When both description and keymap exist: "description | keymap"
        let header = format_header(Some("my_model"), "`ctrl-x`: clear all");
        assert_eq!(header, "my_model | `ctrl-x`: clear all");
    }

    #[test]
    fn test_format_header_description_only() {
        let header = format_header(Some("my_model"), "");
        assert_eq!(header, "my_model");
    }

    #[test]
    fn test_format_header_keymap_only() {
        let header = format_header(None, "`ctrl-x`: clear all");
        assert_eq!(header, "`ctrl-x`: clear all");
    }
}
