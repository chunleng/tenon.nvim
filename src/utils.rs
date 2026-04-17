use std::sync::{
    LazyLock,
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

pub static GLOBAL_EXECUTION_HANDLER: LazyLock<NeovimExecutionHandler> =
    LazyLock::new(|| NeovimExecutionHandler::new());

#[derive(Clone)]
pub struct NeovimExecutionHandler {
    handle: AsyncHandle,
    async_handle: AsyncHandle,
    sender: Sender<(String, Sender<String>)>,
    async_sender: Sender<(String, Sender<String>)>,
}

impl NeovimExecutionHandler {
    pub fn new() -> Self {
        let (tx, rx) = mpsc::channel::<(String, Sender<String>)>();
        let (async_tx, async_rx) = mpsc::channel::<(String, Sender<String>)>();

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

        Self {
            handle,
            async_handle,
            sender: tx,
            async_sender: async_tx,
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

        self.async_sender
            .send((lua_code.to_string(), tx))
            .unwrap();
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
}

