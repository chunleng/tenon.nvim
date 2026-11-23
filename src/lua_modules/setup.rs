use nvim_oxi::{
    Dictionary, Function, Object,
    api::types::LogLevel,
    serde::{DeserializeError, Deserializer},
};

use crate::{
    CONFIG,
    config::{TenonConfig, user::TenonUserConfig},
    lua_modules::entries,
    utils::notify,
};

/// Initialize the plugin configuration. Validation failures come back as a
/// tagged error table rather than a raised Lua error, because raising would
/// unwind through this extern "C" frame and the Rust ABI turns that into a
/// process abort. A second call warns and returns nil, leaving the first
/// config in place.
pub fn setup_fn() -> Function<Object, Object> {
    Function::from_fn_mut(|conf: Object| {
        if CONFIG.get().is_some() {
            notify(
                "[tenon.nvim] setup() called after config already initialized; ignoring",
                LogLevel::Warn,
            );
            return Object::nil();
        }
        match deserialize_user_config(conf).and_then(TenonConfig::try_from) {
            Err(e) => {
                let mut res = Dictionary::new();
                res.insert("ok", false);
                res.insert("error", e.to_string());
                res.into()
            }
            Ok(config) => {
                let _ = CONFIG.set(config);
                let mut res = Dictionary::new();
                res.insert("ok", true);
                res.insert("entries", entries());
                res.into()
            }
        }
    })
}

/// Structural errors carry the serde field chain of the offending value.
/// Paths stop at `#[serde(flatten)]` boundaries (DirectiveConfig.source):
/// errors past a flatten point carry only their inner message.
fn deserialize_user_config(conf: Object) -> Result<TenonUserConfig, nvim_oxi::Error> {
    match serde_path_to_error::deserialize(Deserializer::new(conf)) {
        Ok(v) => Ok(v),
        Err(e) => Err(nvim_oxi::Error::Deserialize(DeserializeError::Custom {
            msg: format!("{}: {}", e.path(), e.inner()),
        })),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nvim_oxi::Array;

    #[test]
    fn deserialize_error_includes_field_path() {
        let agent = Dictionary::from_iter([
            ("model", Object::from(123i64)),
            (
                "directive",
                Object::from(Array::from_iter(Vec::<Object>::new())),
            ),
            (
                "tool_names",
                Object::from(Array::from_iter(Vec::<Object>::new())),
            ),
            ("default", Object::from(true)),
            (
                "choreos",
                Object::from(Array::from_iter(Vec::<Object>::new())),
            ),
        ]);
        let conf = Dictionary::from_iter([(
            "agents",
            Object::from(Dictionary::from_iter([("main", Object::from(agent))])),
        )]);

        let err = deserialize_user_config(Object::from(conf))
            .err()
            .expect("invalid type for agents.main.model should reject");

        assert!(err.to_string().contains("agents.main.model"), "got: {err}");
    }

    #[test]
    fn deserialize_error_names_empty_directive_element() {
        let agent = Dictionary::from_iter([
            (
                "model",
                Object::from(Dictionary::from_iter([("id", Object::from("fast"))])),
            ),
            (
                "directive",
                Object::from(Array::from_iter([Object::from(Dictionary::new())])),
            ),
        ]);
        let conf = Dictionary::from_iter([(
            "agents",
            Object::from(Dictionary::from_iter([("main", Object::from(agent))])),
        )]);

        let err = deserialize_user_config(Object::from(conf))
            .err()
            .expect("empty dictionary as a directive element should reject");

        assert!(
            err.to_string().contains("agents.main.directive[0]"),
            "got: {err}"
        );
    }

    #[test]
    fn deserialize_error_names_unknown_hook_variant() {
        let hook = Dictionary::from_iter([
            ("name", Object::from("notify")),
            ("command", Object::from("echo done")),
            ("type", Object::from("bogus")),
        ]);
        let conf = Dictionary::from_iter([(
            "hooks",
            Object::from(Array::from_iter([Object::from(hook)])),
        )]);

        let err = deserialize_user_config(Object::from(conf))
            .err()
            .expect("unknown hook type variant should reject during deserialization");

        assert!(err.to_string().contains("hooks[0].type"), "got: {err}");
    }
}
