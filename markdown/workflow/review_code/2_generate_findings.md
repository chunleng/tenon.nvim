## Process
1. Gather the full diff using the diff source configuration from workflow step 1 (current state against base). On re-entry from workflow step 3, re-gather the full diff so the user's fixes are seen in context.
2. Review only the changed code (the diff), not unchanged code
3. Check all categories:
   - **Correctness** → Logic errors, race conditions, null/edge cases, crashes, environment coupling
   - **Security** → Injection, auth bypasses, data exposure, input validation
   - **Clarity** → Unclear naming, missing context, magic numbers
   - **Maintainability** → Tight coupling, fragile interfaces, god objects
   - **Style** → Internal inconsistencies only (not style guide violations)
4. For each finding, note the category, title, file path, and line numbers

## Workflow Step Artifact
Raw findings across all categories:
```yaml
findings:
  - category: Correctness | Security | Clarity | Maintainability | Style
    title: Short description of the issue
    file: File path
    lines: Line number(s), ranges as L45-52, single as L78
```
