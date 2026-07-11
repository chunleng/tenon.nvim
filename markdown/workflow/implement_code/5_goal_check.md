## Purpose
Determine if goal is reached

## Process
Compare current state with requirements from Understand step (requirements persist throughout workflow):
- Check each acceptance criterion from Understand step
- Verify all criteria are met

If goal not reached:
- Identify remaining gap
- Explain what's still needed
- Return to Plan step for next iteration

If goal reached:
- Confirm all acceptance criteria satisfied
- Document reasoning
- Proceed to Cleanup step

Goal reached criteria:
- All acceptance criteria met
- All verification methods passed
- No remaining work from requirements

## Workflow Step Output
```yaml
goal_reached: true|false
reasoning: "explanation of decision"
remaining_gap: "what's still needed (if goal_reached: false)"
```

## Example
```yaml
goal_reached: true
reasoning: "Empty password validation added, all acceptance criteria met, tests passing"
```

```yaml
goal_reached: false
reasoning: "Empty password check added but error message not user-friendly"
remaining_gap: "Add user-friendly error message for empty password validation"
```
