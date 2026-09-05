## Process
Choreos are for Tenon internal developer use only. Paths are fixed:
- Config path: `src/chat/choreo`
- Storage path base: `markdown/choreo` (value of `CHOREO_BASE` in `src/chat/choreo/mod.rs`)

This move only sets up paths and determines choreo_id/mode. Do not design moves or gather requirements — those happen in later moves.

1. Determine the choreo_id — build on what the user already stated; if not clear, ask which choreo they want to create or edit
   - If the user stated a name, use it as-is — do not normalize or validate it against any convention
   - If the user has no idea (no choreo name mentioned), set choreo_id to null and mode to `create` — it will be determined during requirements gathering; if generated, it must follow the `<verb>_*` format (e.g. `review_code`, `edit_choreo`)
2. Check if the choreo_id exists in the config file to determine mode (create vs update)
3. Set storage_path to `markdown/choreo/{choreo_id}` (if choreo_id is null, use `markdown/choreo` as base; will be refined later)

## Choreo Move Artifact
```yaml
config_path: src/chat/choreo
storage_path: markdown/choreo/{choreo_id}
mode: create | update  # create by default if choreo_id is null
choreo_id: ... | null  # null if user has no idea yet
```
