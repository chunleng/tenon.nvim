## Process
1. If re-entered from Review or Draft, focus on the feedback that triggered the re-entry (structural issues from Review, or design-change feedback from Draft)
2. If there's an existing choreo, read its config and all instruction files before designing modifications
3. List all tasks the choreo needs to accomplish (from the requirements)
4. For each task, determine whether it needs its own move using the isolation criteria below
5. Every move must produce an artifact, OR produce a side effect (e.g., edited files), OR exist solely for routing/decision-making (e.g., review moves that check work and route based on findings)
6. Define the move sequence, each move's artifact, and goto_instructions
7. Write the choreo definition to `{config_path}/{choreo_id}.rs`:
   - `id`, `title`, `description`, `moves` (with `title`, instruction file path using `{storage_path}/{N}_{move_name}.md` pattern where N is the move number, e.g. `1_`, `2_`, `3_`), `goto_instructions`
   - Order goto conditions before catch-all (null condition) — null always matches, blocking later conditions
   - Omit implicit goto instructions: Next without condition and without memory artifact is implicit. EndChoreo in the last move without condition is also implicit. Self-loops (goto to the same move, e.g. `GotoMove::Move(3)` from move 3) are also implicit — express re-iteration in the move's Process as "loop back to process step N" instead
8. For each move, summarize its input, what it processes, and its artifact
9. Ask user to confirm `{config_path}/{choreo_id}.rs` before proceeding. If user requests changes, edit the file and loop back to process step 4

## Move Isolation Criteria
For each task, walk this decision tree:

1. Does the result need to persist in choreo memory for a later move?
   - Yes → Separate move (artifact persistence)
   - No → continue
2. Is the task complex or important enough that grouping with other instructions reduces quality?
   - Yes → Separate move (LLM rushes ahead instead of focusing on the important task)
   - No → continue
3. Is the move needed to create a goto branch?
   - Yes → Separate move (navigation branching)
   - No → continue
4. Does the task involve both work and checking that work?
   - Yes → Separate the check into its own move (LLM conflicts when given work and its check together)
   - No → keep together with the current move

## Choreo Memory Usage
- Use choreo memory only for items that remain the focus throughout the choreo (e.g. `requirements`, `move_design`)
- Do NOT use choreo memory for transient items (e.g. review issues) — they persist beyond their useful scope and cause confusion
- Routing itself indicates re-entry; use "if re-entered from Review or Draft" rather than checking choreo memory for transient state

## Choreo Move Artifact
```yaml
navigate_artifact:
  from_<move_number>_to_<move_number>: artifact content description | none
  ...
```
