use nvim_oxi::Result as OxiResult;

use crate::tools::tool_matches_selectors;
use crate::utils::GLOBAL_EXECUTION_HANDLER;

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
    let marked_options: Vec<String> = options
        .iter()
        .map(|opt| {
            if current == Some(*opt) {
                format!("{}{}", CURRENT_MARKER, opt)
            } else {
                format!("{}{}", OTHER_MARKER, opt)
            }
        })
        .collect();

    let options_lua = format!(
        "{{ {} }}",
        marked_options
            .iter()
            .map(|o| format!("[[{}]]", o))
            .collect::<Vec<_>>()
            .join(", ")
    );

    let prompt_escaped = prompt.replace('\'', "\\'");

    let lua_code = format!(
        r#"
local fzf = require('fzf-lua')
local resolved = false

fzf.fzf_exec({options}, {{
    prompt = '{prompt}> ',
    winopts = {{
        on_create = function()
            local winid = vim.api.nvim_get_current_win()
            vim.api.nvim_create_autocmd('WinClosed', {{
                pattern = tostring(winid),
                once = true,
                callback = function()
                    vim.defer_fn(function()
                        if not resolved then
                            resolved = true
                            resolve({{error = "cancelled"}})
                        end
                    end, 3000)
                end,
            }})
        end,
    }},
    actions = {{
        ['default'] = function(sel)
            if not resolved then
                resolved = true
                resolve({{response = sel and sel[1] or nil}})
            end
        end,
    }},
}})
"#,
        options = options_lua,
        prompt = prompt_escaped,
    );

    std::thread::spawn(move || {
        let selected = match GLOBAL_EXECUTION_HANDLER.execute_on_main_thread_async(&lua_code) {
            Ok(result) => {
                if result.get("error").is_some() {
                    None
                } else {
                    result
                        .get("response")
                        .and_then(|v| v.as_str())
                        .map(|s| clean_marker(s))
                }
            }
            Err(_) => None,
        };
        on_select(selected);
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
    // Sort: current tools first, then the rest.
    let mut sorted: Vec<&str> = options
        .iter()
        .filter(|o| tool_matches_selectors(o, current))
        .copied()
        .collect();
    let current_count = sorted.len();
    sorted.extend(
        options
            .iter()
            .filter(|o| !tool_matches_selectors(o, current))
            .copied(),
    );

    let options_lua = format!(
        "{{ {} }}",
        sorted
            .iter()
            .map(|o| format!("[[{}]]", o))
            .collect::<Vec<_>>()
            .join(", ")
    );

    let prompt_escaped = prompt.replace('\'', "\\'");

    // Build fzf keymap: tab always toggles+down, load pre-selects current tools.
    let load_entry = if current_count > 0 {
        let action = "select+down+".repeat(current_count);
        format!("['load'] = '{}',", action.trim_end_matches('+'))
    } else {
        String::new()
    };

    let lua_code = format!(
        r#"
local fzf = require('fzf-lua')
local resolved = false

fzf.fzf_exec({options}, {{
    prompt = '{prompt}> ',
    fzf_opts = {{
        ['--multi'] = '',
        ['--marker'] = '✓',
        ['--header'] = '(use <TAB> to toggle  <ENTER> to confirm)',
    }},
    keymap = {{
        fzf = {{
            ['tab'] = 'toggle+down',
            {load_entry}
        }},
    }},
    winopts = {{
        on_create = function()
            local winid = vim.api.nvim_get_current_win()
            vim.api.nvim_create_autocmd('WinClosed', {{
                pattern = tostring(winid),
                once = true,
                callback = function()
                    vim.defer_fn(function()
                        if not resolved then
                            resolved = true
                            resolve({{error = "cancelled"}})
                        end
                    end, 3000)
                end,
            }})
        end,
    }},
    actions = {{
        ['default'] = function(sel)
            if not resolved then
                resolved = true
                resolve({{response = sel}})
            end
        end,
        ['ctrl-y'] = function()
            if not resolved then
                resolved = true
                resolve({{response = {{}}}})
            end
        end
    }},
}})
"#,
        options = options_lua,
        prompt = prompt_escaped,
        load_entry = load_entry,
    );

    std::thread::spawn(move || {
        let selected = match GLOBAL_EXECUTION_HANDLER.execute_on_main_thread_async(&lua_code) {
            Ok(result) => {
                if result.get("error").is_some() {
                    None
                } else {
                    result
                        .get("response")
                        .and_then(|v| v.as_array().cloned().or(Some(vec![])))
                        .map(|arr| {
                            arr.iter()
                                .filter_map(|item| item.as_str().map(|s| s.to_string()))
                                .collect::<Vec<_>>()
                        })
                }
            }
            Err(_) => None,
        };
        on_select(selected);
    });

    Ok(())
}
