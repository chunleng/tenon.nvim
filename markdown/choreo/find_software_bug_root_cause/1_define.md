## Purpose
Gather info to understand bug trigger

## Process
Never guess behavior. Never assume steps from code reading.

Check known:
- Steps to trigger bug
- Expected behavior
- Actual behavior

If any missing → Insufficient Info
If known special conditions trigger bug → add to output

### Insufficient Info

- Run tools to observe bug
- If impossible → ask user for details

### Observe Bug Via Tool
There are many ways to observe what's happening inside code

- Use existing context: execute code, run tests, inspect logs
- Bug isolation technique

## Choreo Move Artifact
```yaml
- steps_to_reproduce:
    - "step1"
    - "step2"
  conditions: "conditions to reproduce, or 'none'"
  expected_behavior: "expected behavior"
  actual_behavior: "actual behavior"
```

## Example
```yaml
- steps_to_reproduce:
    - "login"
    - "click button A"
  conditions: "user has admin role and browser is Safari"
  expected_behavior: "go to settings page"
  actual_behavior: "user got logged out"
```
