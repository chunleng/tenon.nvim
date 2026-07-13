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
- Proceed to Cleanup step

Goal reached criteria:
- All acceptance criteria met
- All verification methods passed
- No remaining work from requirements

## Workflow Step Output

### Goal Reached
```yaml
goal_reached: true
```

### Goal Not Reached
```yaml
goal_reached: false
remaining_gap:
  - what's still needed
```

## Example
```yaml
goal_reached: true
```

```yaml
goal_reached: false
remaining_gap:
  - Error message for empty password validation not user-friendly
```
