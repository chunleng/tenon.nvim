use std::path::{Path, PathBuf};
use std::sync::{
    LazyLock, OnceLock,
    mpsc::{self, Sender},
};

use nvim_oxi::{
    Result as OxiResult,
    api::{self, types::LogLevel},
    libuv::AsyncHandle,
    mlua::lua,
    schedule,
};
use serde_json::Value;

pub static PLUGIN_ROOT: OnceLock<PathBuf> = OnceLock::new();

/// Convert an absolute path to a relative path (./relative) if within cwd,
/// or return the absolute path if outside cwd.
pub fn format_path_relative(path: &str) -> String {
    std::env::current_dir()
        .ok()
        .and_then(|cwd| {
            let cwd_str = cwd.to_string_lossy();
            path.strip_prefix(cwd_str.as_ref())
                .map(|rest| format!("./{}", rest.trim_start_matches('/')))
        })
        .unwrap_or_else(|| path.to_string())
}

/// Create a `PathBuf` from a string, expanding a leading `~` to `$HOME`.
///
/// Expands `~` alone or `~/...`. Falls back to the original path if `$HOME`
/// is unset. Does not handle `~user` syntax.
pub fn path_from_str(path: &str) -> PathBuf {
    if path == "~" {
        if let Ok(home) = std::env::var("HOME") {
            return PathBuf::from(home);
        }
    } else if let Some(rest) = path.strip_prefix("~/")
        && let Ok(home) = std::env::var("HOME")
    {
        return PathBuf::from(home).join(rest);
    }
    PathBuf::from(path)
}

/// Resolve a path relative to the plugin root directory.
///
/// Returns the absolute path. If `relative` is already absolute, returns it as-is.
pub fn plugin_path(relative: impl AsRef<Path>) -> PathBuf {
    PLUGIN_ROOT
        .get()
        .expect("PLUGIN_ROOT not initialized")
        .join(relative.as_ref())
}

fn escape_lua_string(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
        .replace('\t', "\\t")
        .replace('"', "\\\"")
}

fn log_level_to_lua(log_level: LogLevel) -> &'static str {
    match log_level {
        LogLevel::Error => "vim.log.levels.ERROR",
        LogLevel::Warn => "vim.log.levels.WARN",
        LogLevel::Info => "vim.log.levels.INFO",
        LogLevel::Debug => "vim.log.levels.DEBUG",
        _ => "vim.log.levels.INFO",
    }
}

/// A wrapper for vim.notify that properly handles long lines and multiline messages
///
/// This uses Lua's vim.notify which:
/// - Respects user's notification manager (nvim-notify, noice.nvim, etc.)
/// - Properly handles long lines and multiline messages
/// - Supports log levels with appropriate highlighting
pub fn notify(message: impl ToString, log_level: LogLevel) {
    let msg = message.to_string();
    let lua_level = log_level_to_lua(log_level);
    let escaped = escape_lua_string(&msg);
    let lua_code = format!("lua vim.notify(\"{}\", {})", escaped, lua_level);
    let _ = api::command(&lua_code);
}

/// Format a token count with 2 significant figures and K/M/B suffixes.
///
/// Examples:
/// - 100 → "100"
/// - 1020 → "1.0K"
/// - 15300 → "15K"
/// - 1234567 → "1.2M"
/// - 1500000000 → "1.5B"
pub fn format_token_count(count: u64) -> String {
    if count < 1000 {
        return count.to_string();
    }

    let units = ["K", "M", "B"];
    let mut num = count as f64;
    let mut unit_index = 0;

    while num >= 1000.0 && unit_index < units.len() {
        num /= 1000.0;
        unit_index += 1;
    }

    // unit_index is now 1-based (1=K, 2=M, 3=B)
    let suffix = units[unit_index - 1];

    // Format with up to 1 decimal place, removing trailing zeros
    if num < 10.0 {
        format!("{:.1}{}", num, suffix)
    } else {
        format!("{:.0}{}", num, suffix)
    }
}

/// Convert serde_yaml's double-quoted strings containing `\n` into YAML literal
/// block scalar style (`|`) for better readability by the LLM.
pub fn format_yaml_block_scalars(yaml: &str) -> String {
    let re = regex::Regex::new(r#"(?m)^(\s*(?:- )?)([^:\n]+): "((?:[^"\\]|\\.)*)""#).unwrap();

    re.replace_all(yaml, |caps: &regex::Captures| {
        let prefix = &caps[1];
        let value = &caps[3];

        if !value.contains(r"\n") {
            return caps[0].to_string();
        }

        let unescaped = unescape_yaml_string(value);
        let content_indent = " ".repeat(prefix.len() + 2);
        let block = unescaped
            .split('\n')
            .map(|line| format!("{}{}", content_indent, line))
            .collect::<Vec<_>>()
            .join("\n");

        format!("{}{}: |\n{}", prefix, &caps[2], block)
    })
    .to_string()
}

fn unescape_yaml_string(s: &str) -> String {
    let mut result = String::new();
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some('n') => result.push('\n'),
                Some('t') => result.push('\t'),
                Some('\\') => result.push('\\'),
                Some('"') => result.push('"'),
                Some(other) => {
                    result.push('\\');
                    result.push(other);
                }
                None => result.push('\\'),
            }
        } else {
            result.push(c);
        }
    }
    result
}

/// Format token count with delta, hiding delta when it's 0.
///
/// Examples:
/// - (100, 0) → "100"
/// - (100, 5) → "100 (+5)"
/// - (1020, 100) → "1.0K (+100)"
pub fn format_token_with_delta(accumulated: u64, delta: u64) -> String {
    let accumulated_str = format_token_count(accumulated);
    if delta == 0 {
        accumulated_str
    } else {
        format!("{} (+{})", accumulated_str, format_token_count(delta))
    }
}

pub static GLOBAL_EXECUTION_HANDLER: LazyLock<NeovimExecutionHandler> =
    LazyLock::new(NeovimExecutionHandler::new);

type RustCallback = Box<dyn FnOnce() + Send>;

pub struct NeovimExecutionHandler {
    handle: AsyncHandle,
    async_handle: AsyncHandle,
    rust_handle: AsyncHandle,
    sender: Sender<(String, Sender<String>)>,
    async_sender: Sender<(String, Sender<String>)>,
    rust_sender: Sender<RustCallback>,
}

impl NeovimExecutionHandler {
    pub fn new() -> Self {
        let (tx, rx) = mpsc::channel::<(String, Sender<String>)>();
        let (async_tx, async_rx) = mpsc::channel::<(String, Sender<String>)>();
        let (rust_tx, rust_rx) = mpsc::channel::<RustCallback>();

        let handle = AsyncHandle::new(move || {
            while let Ok((data, tx)) = rx.try_recv() {
                let tx = tx.clone();
                schedule(move |_| {
                    let res = lua().load(data.trim()).eval::<mlua::Value>();
                    match res {
                        Ok(x) => {
                            if let Ok(serialized) = serde_json::to_string(&x) {
                                let _ = tx.send(serialized);
                            }
                        }
                        Err(e) => {
                            notify(format!("{:?}", e), LogLevel::Error);
                        }
                    }
                });
            }
        })
        .unwrap();

        let async_handle = AsyncHandle::new(move || {
            while let Ok((code, result_tx)) = async_rx.try_recv() {
                let result_tx = result_tx.clone();
                schedule(move |_| {
                    let lua = lua();

                    // Create a resolve callback that sends the Lua value back to Rust
                    let tx_clone = result_tx.clone();
                    let resolve = lua.create_function(move |_, value: mlua::Value| {
                        if let Ok(serialized) = serde_json::to_string(&value) {
                            let _ = tx_clone.send(serialized);
                        }
                        Ok(())
                    });

                    match resolve {
                        Ok(resolve_fn) => {
                            // Wrap user code in an IIFE that receives `resolve` as a parameter,
                            // avoiding global pollution and supporting concurrent async calls.
                            let wrapped = format!("(function(resolve) {} end)(...)", code.trim());

                            let res = lua.load(&wrapped).call::<()>(resolve_fn);
                            if let Err(e) = res {
                                notify(format!("{:?}", e), LogLevel::Error);
                            }
                        }
                        Err(e) => {
                            notify(
                                format!("Failed to create resolve callback: {:?}", e),
                                LogLevel::Error,
                            );
                        }
                    }
                });
            }
        })
        .unwrap();

        let rust_handle = AsyncHandle::new(move || {
            while let Ok(callback) = rust_rx.try_recv() {
                schedule(move |_| {
                    callback();
                });
            }
        })
        .unwrap();

        Self {
            handle,
            async_handle,
            rust_handle,
            sender: tx,
            async_sender: async_tx,
            rust_sender: rust_tx,
        }
    }

    /// Execute synchronous Lua code on the main thread and return the result.
    ///
    /// The Lua code should use `return` to send back a value.
    pub fn execute_on_main_thread(&self, lua_code: &str) -> OxiResult<Value> {
        let (tx, rx) = mpsc::channel::<String>();

        self.sender.send((lua_code.to_string(), tx)).unwrap();
        self.handle.send()?;

        rx.recv()
            .map_err(|e| nvim_oxi::Error::Mlua(mlua::Error::RuntimeError(e.to_string())))
            .and_then(|json_str| {
                serde_json::from_str::<Value>(&json_str).map_err(|e| {
                    nvim_oxi::Error::Mlua(mlua::Error::RuntimeError(format!(
                        "Failed to parse JSON: {}",
                        e
                    )))
                })
            })
    }

    /// Execute asynchronous Lua code on the main thread and return the result.
    ///
    /// The Lua code receives a `resolve` callback as a parameter.
    /// Call `resolve(value)` when the async work completes to send the result back.
    ///
    /// # Example Lua code
    /// ```lua
    /// vim.defer_fn(function()
    ///     resolve(vim.fn.getcwd())
    /// end, 0)
    /// ```
    pub fn execute_on_main_thread_async(&self, lua_code: &str) -> OxiResult<Value> {
        let (tx, rx) = mpsc::channel::<String>();

        self.async_sender.send((lua_code.to_string(), tx)).unwrap();
        self.async_handle.send()?;

        rx.recv()
            .map_err(|e| nvim_oxi::Error::Mlua(mlua::Error::RuntimeError(e.to_string())))
            .and_then(|json_str| {
                serde_json::from_str::<Value>(&json_str).map_err(|e| {
                    nvim_oxi::Error::Mlua(mlua::Error::RuntimeError(format!(
                        "Failed to parse JSON: {}",
                        e
                    )))
                })
            })
    }

    pub fn notify_on_main_thread(&self, message: impl Into<String>, log_level: LogLevel) {
        let msg = message.into();
        let lua_level = log_level_to_lua(log_level);
        let escaped = escape_lua_string(&msg);
        let lua_code = format!("vim.notify(\"{}\", {})", escaped, lua_level);
        let _ = self.execute_on_main_thread(&lua_code);
    }

    /// Execute a Rust closure on the main thread and return the result.
    ///
    /// This allows calling nvim-oxi APIs directly from off-thread code.
    /// The closure runs on Neovim's main thread where all API calls are safe.
    ///
    /// # Example
    /// ```rust
    /// let result = GLOBAL_EXECUTION_HANDLER.execute_rust_on_main_thread(|| {
    ///     api::get_current_line()
    /// })?;
    /// ```
    pub fn execute_rust_on_main_thread<F, T>(&self, f: F) -> OxiResult<T>
    where
        F: FnOnce() -> OxiResult<T> + Send + 'static,
        T: serde::Serialize + for<'de> serde::Deserialize<'de>,
    {
        let (tx, rx) = mpsc::channel::<Result<String, String>>();

        let closure = move || match f() {
            Ok(result) => match serde_json::to_string(&result) {
                Ok(json) => {
                    let _ = tx.send(Ok(json));
                }
                Err(e) => {
                    let _ = tx.send(Err(e.to_string()));
                }
            },
            Err(e) => {
                let _ = tx.send(Err(format!("{:?}", e)));
            }
        };

        self.rust_sender.send(Box::new(closure)).unwrap();
        self.rust_handle.send()?;

        rx.recv()
            .map_err(|e| nvim_oxi::Error::Mlua(mlua::Error::RuntimeError(e.to_string())))
            .and_then(|result| {
                result.map_err(|e| nvim_oxi::Error::Mlua(mlua::Error::RuntimeError(e)))
            })
            .and_then(|json_str| {
                serde_json::from_str::<T>(&json_str).map_err(|e| {
                    nvim_oxi::Error::Mlua(mlua::Error::RuntimeError(format!(
                        "Failed to parse JSON: {}",
                        e
                    )))
                })
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_token_count_small() {
        assert_eq!(format_token_count(0), "0");
        assert_eq!(format_token_count(100), "100");
        assert_eq!(format_token_count(999), "999");
    }

    #[test]
    fn test_format_token_count_thousands() {
        assert_eq!(format_token_count(1000), "1.0K");
        assert_eq!(format_token_count(1020), "1.0K");
        assert_eq!(format_token_count(1500), "1.5K");
        assert_eq!(format_token_count(15300), "15K");
        assert_eq!(format_token_count(999999), "1000K");
    }

    #[test]
    fn test_format_token_count_millions() {
        assert_eq!(format_token_count(1_000_000), "1.0M");
        assert_eq!(format_token_count(1_234_567), "1.2M");
        assert_eq!(format_token_count(15_000_000), "15M");
    }

    #[test]
    fn test_format_token_count_billions() {
        assert_eq!(format_token_count(1_000_000_000), "1.0B");
        assert_eq!(format_token_count(1_500_000_000), "1.5B");
    }

    #[test]
    fn test_format_yaml_block_scalars_converts_multiline() {
        let input = r#"test: "a\nb""#;
        let expected = "test: |\n  a\n  b";
        assert_eq!(format_yaml_block_scalars(input), expected);
    }

    #[test]
    fn test_format_yaml_block_scalars_converts_multiline_in_object() {
        let input = r#"result: "line one\nline two\nline three"
count: 5"#;
        let expected = "result: |\n  line one\n  line two\n  line three\ncount: 5";
        assert_eq!(format_yaml_block_scalars(input), expected);
    }

    #[test]
    fn test_path_from_str() {
        let orig_home = std::env::var("HOME").ok();
        unsafe { std::env::set_var("HOME", "/home/testuser") };

        // ~ alone → HOME
        assert_eq!(path_from_str("~").to_str().unwrap(), "/home/testuser");

        // ~/path → HOME/path
        assert_eq!(
            path_from_str("~/projects/foo").to_str().unwrap(),
            "/home/testuser/projects/foo"
        );

        // No leading ~ → unchanged
        assert_eq!(
            path_from_str("/absolute/path").to_str().unwrap(),
            "/absolute/path"
        );

        // ~ in middle → unchanged
        assert_eq!(
            path_from_str("/tmp/~weird").to_str().unwrap(),
            "/tmp/~weird"
        );

        // Relative path → unchanged
        assert_eq!(
            path_from_str("relative/path").to_str().unwrap(),
            "relative/path"
        );

        // ~user (not ~/ or ~ alone) → unchanged
        assert_eq!(
            path_from_str("~otheruser/foo").to_str().unwrap(),
            "~otheruser/foo"
        );

        // No HOME → falls back to original path
        unsafe { std::env::remove_var("HOME") };
        assert_eq!(path_from_str("~/projects").to_str().unwrap(), "~/projects");

        match orig_home {
            Some(h) => unsafe { std::env::set_var("HOME", h) },
            None => {}
        }
    }
}
