## Purpose
Verify plan addresses requirement without unintended side effects

## Process
1. Check each change addresses requirement:
   - Map each acceptance criteria to corresponding changes
   - Verify changes are sufficient to meet requirement
   - Review affected code paths
2. Check for unintended side effects:
   - Trace dependencies from changed components
   - Verify constraints are respected in implementation
   - Check integration points handle edge cases
3. Check for gaps:
   - Verify all integration points are covered
4. If issues found, return to Analyze step

## Workflow Step Output
When verification fails (issues found):
```yaml
status: "verification failed"
issues:
  - issue: "description of problem"
    type: "missing | side_effect | gap"
    suggestion: "how to fix"
```

When verification passes:
```yaml
requirement: "what user wants to achieve"
constraints:
  - "constraint 1"
  - "constraint 2"
acceptance_criteria:
  - criteria: "measurable outcome"
    verification: "how to verify"
changes:
  - target: "file or component"
    description: "what to do"
    rationale: "why this change"
    integration_points:
      - "where it connects"
```
