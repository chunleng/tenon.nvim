## Process
1. Strip out anything that was only there to help you work
  a. Debug prints, temporary variables
  b. Dead code from abandoned approaches
  c. What's left should be only what the goal required
2. Run the full test suite and any project checks (lint, format, type check)
3. If tests fail, output `failures` — this routes back to planning
4. If tests pass, surface any unverifiable items accumulated during Verify steps (from workflow memory) as the final output to the user

## Workflow Step Output

### If tests failed
```yaml
failures:
  - "what failed and why"
```

### If tests passed (workflow ends)
```yaml
unverifiable:
  - "items accumulated during Verify steps, from workflow memory"
```
This is the final output the user sees.
