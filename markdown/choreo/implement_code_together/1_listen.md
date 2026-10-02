## Process
Information-gathering only, no code implementation.
1. Determine what user wants:
  - Make clever assumptions:
    - First round (no `goal` in choreo memory): what user specified before the choreo starts, only if development-goal related
    - Returning round (`goal` in choreo memory, after implementation confirmed): ignore the previous cycle's `goal`. Gather candidates: side work queued during the previous cycle, plus unimplemented ideas from the conversation. State the candidates in chat (if any) and ask: "What to implement next?"
  - No assumptions can be made (no goal-related request before choreo start, or nothing to suggest) → ask: "What to implement next?"
2. Iterate until the goal is clear
  a. Requirement clarity
    i. Research codebase if needed. Ask if can't be found
    ii. Use common defaults. Ask if debatable
  b. Implementation clarity
    i. No existing pattern, e.g. new module → suggest implementation method
    ii. Multiple distinct approaches possible → present alternatives and clarify
3. Show goal and confirm with user: "Please confirm the goal"
  a. User confirmed → next move
  b. Anything other than confirmation → loop to process step 2

## Choreo Move Artifact
```yaml
goal: clear description of the incremental goal to achieve
sidenotes:
  - additional information, constraints
```
