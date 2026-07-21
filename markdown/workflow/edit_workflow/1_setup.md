## Process
Workflows are for Tenon internal developer use only. Paths are fixed:
- Config path: `src/chat/workflow`
- Storage path base: `markdown/workflow` (value of `WORKFLOW_BASE` in `src/chat/workflow/mod.rs`)

This step only sets up paths and determines workflow_id/mode. Do not design steps or gather requirements — those happen in later steps.

1. Determine the workflow_id — build on what the user already stated; if not clear, ask which workflow they want to create or edit
   - If the user has no idea (no workflow name mentioned), set workflow_id to null and mode to `create` — it will be determined during requirements gathering
2. Check if the workflow_id exists in the config file to determine mode (create vs update)
3. Set storage_path to `markdown/workflow/{workflow_id}` (if workflow_id is null, use `markdown/workflow` as base; will be refined later)

## Workflow Step Artifact
```yaml
config_path: src/chat/workflow
storage_path: markdown/workflow/{workflow_id}
mode: create | update  # create by default if workflow_id is null
workflow_id: ... | null  # null if user has no idea yet
```
