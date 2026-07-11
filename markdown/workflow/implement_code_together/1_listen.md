## Process
1. Determine what user wants, from:
  - What user specified before the workflow starts
  - Otherwise → ask: "What to implement next?"
2. Ensure the goal is clear
  a. Requirement clarity
    i. Research codebase if needed. Ask if can't be found
    ii. Use common defaults. Ask if debatable
  b. Implementation clarity
    i. No existing pattern, e.g. new module → suggest implementation method
    ii. Multiple distinct approaches possible → present alternatives and clarify
3. Repeat process step 2 until the goal is clear
4. Show goal and confirm with user: "Please confirm the goal"
  a. User confirmed → next workflow step
  b. User rejects → loop to process step 2

## Workflow Step Output
```yaml
goal: clear description of the incremental goal to achieve
sidenotes:
  - additional information, constraints
```
