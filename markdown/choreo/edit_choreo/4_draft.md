## Process
1. If re-entered from Review, focus fixes on the content issues flagged in the review
2. For each move in the design, draft instruction content following this format:
   - **Process**: Numbered steps to execute
   - **Choreo Move Artifact**: What this move provides as its artifact, in YAML format (per the design). If the move has no artifact, drop this section entirely.
   - If the Choreo Move Artifact varies by goto path (e.g. different artifacts for structural vs content routing), use subsections for each variant
3. Write instruction files to `{storage_path}/{N}_{move_name}.md` where N is the move number, e.g. `1_`, `2_`, `3_`
4. If creating, add the new module to `{config_path}/mod.rs`
5. Present summary of changes and ask user for confirmation
   - If user requests content-only changes, loop back to process step 2
   - If user feedback involves move design changes, navigate to move 3

## Drafting Guidelines
- Clear, minimal language
- Include examples when the instruction is abstract or complex
- Use generic examples (not tied to specific tools/frameworks)
- Do not use "output" to mean "artifact" — the agent will confuse it with printing to chat. Use "provide" or "artifact" instead
- Do not include routing in instruction files unless necessary. Routing is only needed mid-process to invoke a navigation check during processing (e.g. "if X → go to move Y"). Do NOT put routing at the end or as a separate section — this is defined in moves
- Do not include explicit steps to provide the Choreo Move Artifact or to proceed to the next move. The Choreo Move Artifact section (including conditional variants) defines what the move provides; the harness handles artifact provision and navigation

## Choreo Move Artifact
### Default
No artifact

### If user feedback involves move design changes
```yaml
move_design_changes: |
  <the user's feedback about move design>
```
