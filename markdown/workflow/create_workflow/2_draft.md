## Purpose
Draft workflow structure and each step's instruction content

## Process
1. Create workflow metadata (id, title, default_condition)
2. For each step from previous step, create instruction content
3. Write to files:
   - `{storage_path}/{workflow_id}/1_{step_name}.md`
   - `{storage_path}/{workflow_id}/2_{step_name}.md`
   - etc
4. Update `{config_path}/workflow.rs` with new workflow definition

## Drafting Guidelines
- Each step should have: Purpose, Process, Output
- Clear, minimal language
- Include examples when instruction abstract or complex
- Define goto_instructions with conditions and outputs

## Output
Workflow files created + workflow.rs updated
