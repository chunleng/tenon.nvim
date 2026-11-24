#[cfg(not(test))]
use super::helpers::runtime;

#[cfg(not(test))]
fn skill_description(
    skill_id: &str,
    engine: &crate::agent::engine::AgenticStreamEngine,
) -> Option<String> {
    engine
        .choreos()
        .iter()
        .find(|choreo| choreo.id == skill_id)
        .map(|choreo| choreo.description.to_owned())
}

#[nvim_oxi::test]
fn startup_discovers_skills_only_for_enabled_agents() -> Result<(), nvim_oxi::mlua::Error> {
    #[cfg(not(test))]
    {
        use crate::chat::ChatSession;

        let _nvim = runtime::NeovimTestSession::new();
        runtime::write_skill(".agents/skills", "review", "Review changes", "Private body");
        std::fs::rename(".agents/skills/review", ".agents/skills/review-files").unwrap();
        runtime::write_skill(".agents/skills", "zebra", "Zebra skill", "");
        runtime::write_skill(".claude/skills", "alpha", "Alpha skill", "");
        runtime::write_skill(".claude/skills", "review", "Lower priority", "");
        std::fs::rename(".claude/skills/review", ".claude/skills/old-review").unwrap();
        runtime::configure_agents(serde_json::json!({
                "disabled": {"model": {"id": "fast"}, "default": true},
                "enabled": {"model": {"id": "fast"}, "project_skills": true}
        }));

        let disabled = ChatSession::with_agent_name("disabled".into()).unwrap();
        assert!(skill_description("review", &disabled.engine).is_none());

        let enabled = ChatSession::with_agent_name("enabled".into()).unwrap();
        let catalog = enabled.engine.choreos();
        let listed: Vec<_> = catalog
            .project
            .iter()
            .map(|skill| {
                (
                    skill.name.as_str(),
                    skill.description.as_str(),
                    skill.base_path.as_ref(),
                )
            })
            .collect();
        assert_eq!(
            listed,
            [
                (
                    "alpha",
                    "Alpha skill",
                    std::path::Path::new(".claude/skills/alpha")
                ),
                (
                    "review",
                    "Review changes",
                    std::path::Path::new(".agents/skills/review-files")
                ),
                (
                    "zebra",
                    "Zebra skill",
                    std::path::Path::new(".agents/skills/zebra")
                ),
            ]
        );
    }
    Ok(())
}

#[nvim_oxi::test]
fn invalid_skill_warns_and_falls_back_to_valid_duplicate() -> Result<(), nvim_oxi::mlua::Error> {
    #[cfg(not(test))]
    {
        let nvim = runtime::NeovimTestSession::new();
        std::fs::create_dir_all(".agents/skills/review").unwrap();
        runtime::write_skill(".claude/skills", "review", "Valid fallback", "");
        std::fs::rename(".claude/skills/review", ".claude/skills/fallback").unwrap();

        let skills = crate::chat::project_skills::discover();
        let notices = nvim.take_notices()?;

        assert_eq!(skills.len(), 1);
        assert_eq!(skills[0].description, "Valid fallback");
        assert_eq!(
            skills[0].base_path.as_ref(),
            std::path::Path::new(".claude/skills/fallback")
        );
        assert_eq!(notices.len()?, 1);
        let notice: nvim_oxi::mlua::Table = notices.get(1)?;
        let message: String = notice.get("message")?;
        assert!(message.contains(".agents/skills/review/SKILL.md"));
        let warn: i64 = nvim_oxi::mlua::lua()
            .load("return vim.log.levels.WARN")
            .eval()?;
        assert_eq!(notice.get::<i64>("level")?, warn);
    }
    Ok(())
}

#[nvim_oxi::test]
fn unreadable_skill_root_warns_and_other_root_is_discovered() -> Result<(), nvim_oxi::mlua::Error> {
    #[cfg(not(test))]
    {
        let nvim = runtime::NeovimTestSession::new();
        runtime::write_skill(".claude/skills", "review", "Available skill", "");

        let skills = crate::chat::project_skills::discover();
        let notices = nvim.take_notices()?;
        assert_eq!(skills.len(), 1);
        assert_eq!(notices.len()?, 0);

        std::fs::create_dir_all(".agents").unwrap();
        std::fs::write(".agents/skills", "not a directory").unwrap();
        let skills = crate::chat::project_skills::discover();
        let notices = nvim.take_notices()?;

        assert_eq!(skills.len(), 1);
        assert_eq!(skills[0].name, "review");
        assert_eq!(notices.len()?, 1);
        let notice: nvim_oxi::mlua::Table = notices.get(1)?;
        let message: String = notice.get("message")?;
        assert!(message.contains(".agents/skills"));
        let warn: i64 = nvim_oxi::mlua::lua()
            .load("return vim.log.levels.WARN")
            .eval()?;
        assert_eq!(notice.get::<i64>("level")?, warn);
    }
    Ok(())
}

#[cfg(not(test))]
use crate::chat::{
    ChatSession,
    history::{ChatHistory, SessionMetadata, load_history_entries, save_to_history},
};

#[cfg(not(test))]
fn save_session(session: &ChatSession) -> ChatHistory {
    save_to_history(
        SessionMetadata {
            id: &session.id,
            title: None,
            agent_name: &session.active_agent_name,
            model_display: "llama3",
            session_datetime: session.session_datetime,
        },
        &session.engine.log_handler.log_window.read().unwrap(),
        &session.usage,
        &session.work_queue,
    );
    load_session_history(&session.id)
}

#[cfg(not(test))]
fn load_session_history(id: &str) -> ChatHistory {
    load_history_entries()
        .into_iter()
        .find(|history| history.id == id)
        .unwrap()
}

#[cfg(not(test))]
fn activate_skill(session: &mut ChatSession, id: &str) {
    use crate::tools::{TenonTool, use_choreo::UseChoreo};
    use rig::tool::DynamicTool;

    let tool: DynamicTool = TenonTool::new(
        UseChoreo {
            choreos: session.engine.choreos(),
            active_choreo: session.engine.active_choreo.clone(),
        },
        session.engine.log_handler.log_window.clone(),
    )
    .into();
    futures::executor::block_on(tool.run_with(
        &mut session.engine.tool_context,
        serde_json::json!({"choreo_id": id}),
    ))
    .unwrap();
}

#[nvim_oxi::test]
fn restoring_skill_with_missing_file_warns_and_ends() -> Result<(), nvim_oxi::mlua::Error> {
    #[cfg(not(test))]
    {
        let nvim = runtime::NeovimTestSession::new();
        runtime::write_skill(".agents/skills", "review", "Review changes", "Private body");
        runtime::configure_agents(serde_json::json!({
            "enabled": {"model": {"id": "fast"}, "default": true, "project_skills": true}
        }));
        let mut session = ChatSession::with_agent_name("enabled".into()).unwrap();
        activate_skill(&mut session, "review");
        let history = save_session(&session);
        std::fs::remove_file(".agents/skills/review/SKILL.md").unwrap();

        let mut restored = ChatSession::from_history(history).unwrap();
        futures::executor::block_on(restored.engine.request_messages("Continue".into()));
        let notices = nvim.take_notices()?;

        assert!(restored.engine.active_choreo.read().unwrap().is_none());
        assert!(notices.len()? >= 1);
        let notice: nvim_oxi::mlua::Table = notices.get(1)?;
        let message: String = notice.get("message")?;
        assert!(message.contains(".agents/skills/review/SKILL.md"));
        let warn: i64 = nvim_oxi::mlua::lua()
            .load("return vim.log.levels.WARN")
            .eval()?;
        assert_eq!(notice.get::<i64>("level")?, warn);
    }
    Ok(())
}

#[nvim_oxi::test]
fn switching_agents_updates_available_skill_choreos() -> Result<(), nvim_oxi::mlua::Error> {
    #[cfg(not(test))]
    {
        use crate::chat::ChatSession;

        let _nvim = runtime::NeovimTestSession::new();
        runtime::write_skill(".claude/skills", "review", "Review staged changes", "");
        runtime::configure_agents(serde_json::json!({
                "disabled": {"model": {"id": "fast"}, "default": true},
                "enabled": {"model": {"id": "fast"}, "project_skills": true}
        }));

        let mut session = ChatSession::with_agent_name("disabled".into()).unwrap();
        let config = crate::get_application_config();
        session.switch_agent("enabled".into(), &config.agents["enabled"]);
        assert!(
            skill_description("review", &session.engine)
                .unwrap()
                .contains("Review staged changes")
        );

        session.switch_agent("disabled".into(), &config.agents["disabled"]);
        assert!(skill_description("review", &session.engine).is_none());
    }
    Ok(())
}
