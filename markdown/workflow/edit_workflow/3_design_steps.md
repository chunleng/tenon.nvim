## Process
1. If re-entered from Review or Draft, focus on the feedback that triggered the re-entry (structural issues from Review, or design-change feedback from Draft)
2. If there's an existing workflow, read its config and all instruction files before designing modifications
3. List all tasks the workflow needs to accomplish (from the requirements)
4. For each task, determine whether it needs its own step using the isolation criteria below
5. Every step must produce an artifact, OR produce a side effect (e.g., edited files), OR exist solely for routing/decision-making (e.g., review steps that check work and route based on findings)
6. Define the step sequence, each step's artifact, and goto_instructions
7. Write the workflow definition to `{config_path}/{workflow_id}.rs`:
   - `id`, `title`, `description`, `steps` (with `title`, instruction file path using `{storage_path}/{N}_{step_name}.md` pattern where N is the workflow step number, e.g. `1_`, `2_`, `3_`), `goto_instructions`
   - Order goto conditions before catch-all (null condition) — null always matches, blocking later conditions
   - Omit implicit goto instructions: Next without condition and without memory artifact is implicit. EndWorkflow in the last step without condition is also implicit. Self-loops (goto to the same step, e.g. `GotoStep::Step(3)` from workflow step 3) are also implicit — express re-iteration in the step's Process as "loop back to process step N" instead
8. For each step, write 1 line describing its input, what it processes, and its artifact
9. Ask user to confirm `{config_path}/{workflow_id}.rs` before proceeding. If user requests changes, edit the file and loop back to process step 4

## Step Isolation Criteria
For each task, walk this decision tree:

1. Does the result need to persist in workflow memory for a later step?
   - Yes → Separate step (artifact persistence)
   - No → continue
2. Is the task complex or important enough that grouping with other instructions reduces quality?
   - Yes → Separate step (LLM rushes ahead instead of focusing on the important task)
   - No → continue
3. Is the step needed to create a goto branch?
   - Yes → Separate step (navigation branching)
   - No → continue
4. Does the task involve both work and checking that work?
   - Yes → Separate the check into its own step (LLM conflicts when given work and its check together)
   - No → keep together with the current step

## Workflow Memory Usage
- Use workflow memory only for items that remain the focus throughout the workflow (e.g. `requirements`, `step_design`)
- Do NOT use workflow memory for transient items (e.g. review issues) — they persist beyond their useful scope and cause confusion
- Routing itself indicates re-entry; use "if re-entered from Review or Draft" rather than checking workflow memory for transient state

## Workflow Step Artifact
```yaml
navigate_artifact:
  from_<step_number>_to_<step_number>: artifact content description | none
  ...
```
