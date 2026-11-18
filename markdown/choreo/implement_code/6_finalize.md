## Process
1. Strip out anything that was only there to help you work
  a. Debug prints, temporary variables
  b. Dead code from abandoned approaches
  c. What's left should be only what the goal required
2. Run the full test suite and any project checks (lint, format, type check)
3. Classify each failure as goal-related or unrelated
  a. Goal-related: the failing test/check covers code written for the goal, or the failure is in an area the goal's changes touched; exclude pre-existing failures unless the goal's changes caused or worsened them
  b. Unrelated: all remaining failures
4. For each goal-related failure:
  a. Determine whether it is actionable or blocked
    - Actionable: a concrete next step can address the failure within the agreed goal and available local setup
    - Blocked: evidence shows the failure cannot be resolved within those constraints; record the evidence and what must change to unblock it. Do not silently redefine the goal
  b. Examples: correcting faulty logic is actionable; a required service unavailable in the local setup is blocked if it cannot be made available; a test exposing a false assumption in the goal is blocked when passing it would require changing the goal
  c. An unsuccessful attempt alone does not establish a blocker; identify a concrete next step or evidence of the blocking constraint

## Artifact

### If actionable goal-related failures exist
```yaml
failures:
  - "actionable goal-related failure: what failed, why, and the concrete next step"
```
Include only actionable goal-related failures.

### If no actionable goal-related failures exist (choreo ends)
```yaml
unverifiable:
  - "items accumulated during Verify moves, from choreo memory"
unrelated_failures:
  - "check failures not related to the goal, with why they are unrelated"
blocked_failures:
  - "blocked goal-related failure, evidence explaining why it cannot be resolved, and what must change to unblock it"
```
Omit categories with no items. This is the final artifact of the choreo.
