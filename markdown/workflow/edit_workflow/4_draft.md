## Process
1. If re-entered from Review, focus fixes on the content issues flagged in the review
2. For each step in the design, draft instruction content following this format:
   - **Process**: Numbered steps to execute
   - **Workflow Step Artifact**: What this step provides as its artifact, in YAML format (per the design). If the step has no artifact, drop this section entirely.
   - If the Workflow Step Artifact varies by goto path (e.g. different artifacts for structural vs content routing), use subsections for each variant
3. Write instruction files to `{storage_path}/{N}_{step_name}.md` where N is the workflow step number, e.g. `1_`, `2_`, `3_`
4. If creating, add the new module to `{config_path}/mod.rs`
5. Present summary of changes and ask user for confirmation
   - If user requests content-only changes, loop back to process step 2
   - If user feedback involves step design changes, provide the user's step design feedback as the workflow step artifact

## Drafting Guidelines
- Clear, minimal language
- Include examples when the instruction is abstract or complex
- Use generic examples (not tied to specific tools/frameworks)
- Always say "process step #" or "workflow step #", never just "step #" — the bare form is ambiguous
- Do not use "output" to mean "artifact" — the agent will confuse it with printing to chat. Use "provide" or "artifact" instead
- Do not include routing in instruction files unless necessary. Routing is only needed mid-process to invoke a navigation check during processing (e.g. "if X → go to workflow step Y"). Do NOT put routing at the end or as a separate section — this is defined in workflow steps

## Workflow Step Artifact
### Default
No artifact

### If user feedback involves step design changes
```yaml
step_design_changes: |
  <the user's feedback about step design>
```
