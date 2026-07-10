## Purpose
Verify refactoring plan is sound before implementation

## Process
1. Check each change preserves behavior:
   - No API changes without explicit constraint relaxation
   - No functionality removal
2. Validate changes are complete:
   - All changes align with user's requirement
   - All changes respect constraints
3. Check for gaps:
   - Missing files in affected list
   - Unclear descriptions

## Output
When verification fails (issues found):
```yaml
status: "verification failed"
issues:
  - change: "description of problematic change"
    issue: "what's wrong"
    suggestion: "how to fix"
```
This triggers return to Plan step to address issues.

When verification passes (no issues):
```yaml
status: "passed"
plan: "complete refactoring plan ready for implement_software workflow"
```
This ends the workflow.
