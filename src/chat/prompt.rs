use crate::chat::ActiveWorkflow;
use std::sync::{Arc, RwLock};

/// Builds a workflow-wrapped prompt if there's an active workflow.
pub async fn build_workflow_prompt(
    active_workflow: &Arc<RwLock<Option<ActiveWorkflow>>>,
    base_prompt: String,
) -> String {
    if let Ok(active_lock) = active_workflow.read()
        && let Some(active) = active_lock.as_ref()
    {
        let workflow = &active.workflow;
        let total_steps = workflow.steps.len();
        if let Some(step) = workflow.steps.get(active.step - 1) {
            let mut goto_lines: Vec<String> = step
                .goto_instructions
                .iter()
                .map(|instr| {
                    let condition = instr
                        .condition
                        .as_ref()
                        .map(|x| format!("{} → ", x))
                        .unwrap_or_default();
                    let target_step = instr.to.resolve_step_index(active.step);
                    match target_step {
                        None => format!("{}end_workflow", condition),
                        Some(step) if step > total_steps => {
                            format!("{}end_workflow", condition)
                        }
                        Some(step) => {
                            format!("{}navigate_workflow step:{}", condition, step)
                        }
                    }
                })
                .collect();

            // Only add default ending if at last step and no goto already ends workflow
            if active.step == total_steps {
                let has_ending_goto = step.goto_instructions.iter().any(|instr| {
                    let target_step = instr.to.resolve_step_index(active.step);
                    match target_step {
                        None => true,
                        Some(s) => s > total_steps,
                    }
                });
                if !has_ending_goto {
                    goto_lines.push("end_workflow".to_string());
                }
            }

            let goto_instruction = goto_lines.join("\n");

            // Build memory section if there's stored memory
            let memory_section = if active.memory.is_empty() {
                String::new()
            } else {
                let memory_entries: Vec<String> = active
                    .memory
                    .iter()
                    .map(|(k, v)| format!("<memory name=\"{}\">{}</memory>", k, v))
                    .collect();
                memory_entries.join("\n")
            };

            return format!(
                "<context>\n\
                    Currently in {} step of {} workflow.\n\
                    Execute \"Process\" in `instruction` tag step by step if numbered, not all at once; don't stop partway unless explicitly asked. \
                    Call a tool from `navigation` tag when the condition matches, using artifact from the \"Workflow Step Artifact\" section\n\
                    \n\n\
                    {}\
                    <instruction>\n\
                    {}\n\
                    </instruction>\n\
                    <navigation>\n\
                    {}\n\
                    </navigation>\n\
                    </context>\n\
                    {}",
                step.title,
                workflow.title,
                memory_section,
                step.instruction.resolve().unwrap_or_default(),
                goto_instruction,
                base_prompt
            );
        }
    }

    base_prompt
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[tokio::test]
    async fn test_build_workflow_prompt_displays_memory() {
        crate::utils::PLUGIN_ROOT
            .set(std::env::current_dir().unwrap())
            .ok();

        let registry = crate::get_workflow_registry();
        let wf = registry.get("implement_code").unwrap().clone();

        let workflow = Arc::new(RwLock::new(Some(ActiveWorkflow {
            workflow: wf,
            step: 1,
            memory: {
                let mut m = HashMap::new();
                m.insert("previous_output".to_string(), "test result".to_string());
                m
            },
        })));

        let prompt = build_workflow_prompt(&workflow, "user input".to_string()).await;

        assert!(prompt.contains("<memory name=\"previous_output\">"));
        assert!(prompt.contains("test result"));
        assert!(prompt.contains("</memory>"));
    }

    #[tokio::test]
    async fn test_build_workflow_prompt_no_workflows() {
        crate::utils::PLUGIN_ROOT
            .set(std::env::current_dir().unwrap())
            .ok();

        let workflow = Arc::new(RwLock::new(None));
        let prompt = build_workflow_prompt(&workflow, "user input".to_string()).await;
        assert_eq!(prompt, "user input");
        assert!(!prompt.contains("<context>"));
    }
}
