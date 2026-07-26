## Process
1. Gather the full diff using the diff source configuration from workflow step 1. On re-entry from workflow step 3:
   a. Re-gather the full diff
   b. Read the prior `review_state` memory
2. Review only the changed code (the diff), not unchanged code
3. Check all categories:
   - **Correctness** → Logic errors, race conditions, null/edge cases, crashes, environment coupling
   - **Security** → Injection, auth bypasses, data exposure, input validation
   - **Clarity** → Unclear naming, missing context, magic numbers
   - **Maintainability** → Tight coupling, fragile interfaces, god objects
   - **Style** → Internal inconsistencies only (not style guide violations)
4. For each finding, note the category, title, file path, and line numbers. On re-entry, for findings carried over from `review_state`:
   - `dropped`: keep as-is, do not re-examine
   - `resolved-via-code`: remove if confirmed fixed in the updated diff, otherwise revert to `pending`
   - `pending`: carry forward as-is

## Workflow Step Artifact
```yaml
findings:
  - category: Correctness | Security | Clarity | Maintainability | Style
    title: Short description of the issue
    file: File path
    lines: Line number(s), ranges as L45-52, single as L78
    decision: dropped | pending  # only present on re-entry
```
