use std::path::{Path, PathBuf};

pub struct NeovimTestSession {
    original_cwd: PathBuf,
    root: PathBuf,
    original_notify: nvim_oxi::mlua::Function,
    notice_state: nvim_oxi::mlua::Table,
}

impl NeovimTestSession {
    pub fn new() -> Self {
        let lua = nvim_oxi::mlua::lua();
        let vim: nvim_oxi::mlua::Table = lua.globals().get("vim").unwrap();
        let original_notify = vim.get("notify").unwrap();
        let notice_state = lua.create_table().unwrap();

        let original_cwd = std::env::current_dir().unwrap();
        let root = std::env::temp_dir().join(format!("tenon-nvim-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&root).unwrap();
        let session = Self {
            original_cwd,
            root,
            original_notify,
            notice_state,
        };
        session.capture_notices().unwrap();
        std::env::set_current_dir(&session.root).unwrap();
        crate::utils::PLUGIN_ROOT
            .set(session.original_cwd.clone())
            .ok();
        session
    }

    fn capture_notices(&self) -> Result<(), nvim_oxi::mlua::Error> {
        let lua = nvim_oxi::mlua::lua();
        self.notice_state.set("notices", lua.create_table()?)?;
        let capture: nvim_oxi::mlua::Function = lua
            .load(
                r#"
                return function(state)
                    return function(message, level)
                        table.insert(state.notices, { message = message, level = level })
                    end
                end
                "#,
            )
            .eval::<nvim_oxi::mlua::Function>()?
            .call(self.notice_state.clone())?;
        let vim: nvim_oxi::mlua::Table = lua.globals().get("vim")?;
        vim.set("notify", capture)
    }

    pub fn take_notices(&self) -> Result<nvim_oxi::mlua::Table, nvim_oxi::mlua::Error> {
        let notices = self.notice_state.get("notices")?;
        self.notice_state
            .set("notices", nvim_oxi::mlua::lua().create_table()?)?;
        Ok(notices)
    }
}

impl Drop for NeovimTestSession {
    fn drop(&mut self) {
        let notify_result = nvim_oxi::mlua::lua()
            .globals()
            .get::<nvim_oxi::mlua::Table>("vim")
            .and_then(|vim| vim.set("notify", self.original_notify.clone()));
        let result = std::env::set_current_dir(&self.original_cwd)
            .and_then(|()| std::fs::remove_dir_all(&self.root));
        if let Err(error) = notify_result {
            if std::thread::panicking() {
                eprintln!("failed to restore vim.notify: {error}");
            } else {
                panic!("failed to restore vim.notify: {error}");
            }
        }
        if let Err(error) = result {
            if std::thread::panicking() {
                eprintln!("failed to clean up Neovim test session: {error}");
            } else {
                panic!("failed to clean up Neovim test session: {error}");
            }
        }
    }
}

pub fn configure_agents(agents: serde_json::Value) {
    use crate::config::{TenonConfig, user::TenonUserConfig};

    let config: TenonUserConfig = serde_json::from_value(serde_json::json!({
        "connectors": {"local": {"type": "ollama"}},
        "models": {"fast": {"connector": "local", "name": "llama3"}},
        "agents": agents
    }))
    .unwrap();
    crate::CONFIG
        .set(TenonConfig::try_from(config).unwrap())
        .unwrap();
}

pub fn write_skill(search_path: &str, name: &str, description: &str, body: &str) {
    let directory = Path::new(search_path).join(name);
    std::fs::create_dir_all(&directory).unwrap();
    let name = serde_json::to_string(name).unwrap();
    let description = serde_json::to_string(description).unwrap();
    let content = format!("---\nname: {name}\ndescription: {description}\n---\n{body}");
    std::fs::write(directory.join("SKILL.md"), content).unwrap();
}
