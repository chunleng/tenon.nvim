## Process
1. Strip out anything that was only there to help you work
  a. Debug prints, temporary variables
  b. Dead code from abandoned approaches
  c. What's left should be only what the goal required
2. Run the full test suite and any project checks (lint, format, type check)
3. Classify each failure as goal-related or unrelated
  a. Goal-related: the failing test/check covers code written for the goal, or the failure is in an area the goal's changes touched
  b. Unrelated: pre-existing failures, or failures in areas the goal's changes did not touch
4. If any goal-related failure exists, provide the `failures` artifact
5. If no goal-related failure exists, the choreo ends; surface any unverifiable items accumulated during Verify moves (from choreo memory) and any unrelated failures as the final output to the user

## Choreo Move Artifact

### If goal-related failures exist
```yaml
failures:
  - "what failed and why"
```

### If no goal-related failures (choreo ends)
```yaml
unverifiable:
  - "items accumulated during Verify moves, from choreo memory"
unrelated_failures:
  - "check failures not related to the goal, with why they are unrelated"
```
This is the final output the user sees.
