## Purpose
Review flagged lines and decide whether to replace or remove

## Process
If no flagged lines input, skip this step (pass workflow unchanged to next step)

For each flagged line:
1. Read line in context (surrounding lines)
2. Decide action:
   - **Remove**: Line provides no value or is redundant
   - **Replace**: Line has value but needs rewording
3. Apply changes to file
4. Document changes made
5. Process next line or end

## Decision Guide
- Vague + can't clarify → remove
- Vague + can clarify → replace with specific version
- Redundant → remove
- Contradictory → resolve conflict, update both lines

## Workflow Step Output
```yaml
changes:
  - file: "..."
    line: 5
    action: "removed"
    reason: "..."
  - file: "..."
    line: 12
    action: "replaced"
    old: "..."
    new: "..."
```
