Ask user for next incremental goal. Information-gathering only — no code implementation.

## Process
1. Determine what user wants, this can come from:
  - What user specified or mentioned before the workflow starts
  - Otherwise → ask: "What to implement next?"
2. Ensure what user want is clear as a goal
  a. Requirement clarity
    i. Research code base to understand if needed. Ask if can't be found
    ii. Use default that people would commonly agree. Ask if it is debatable
  b. Implementation clarity
    i. No existing pattern for implementation, e.g. totally new module with no prior code to follow → suggest 
    ii. Multiple significantly different approaches possible → present alternatives and clarify
3. Perform process step 2 until the goal is clear
4. Output goal and confirm with user, say "Please confirm the goal"
  a. User confirmed → next workflow step
  b. User reject → loop to process step 2: clarify goal

## Output
```yaml
goal: "clear description of the incremental goal user wants to achieve"
sidenotes:
  - "any additional context not covered by the goal"
```

## Example
```yaml
goal: "Add empty string validation to password field"
sidenotes:
  - "Should return user-friendly error message"
```
