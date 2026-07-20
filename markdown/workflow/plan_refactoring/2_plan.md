## Purpose
Analyze code and create refactoring plan

## Process
1. Identify files involved based on requirement
2. Analyze each file (dependencies, callers, exports)
3. Design changes that respect constraints

## Workflow Step Artifact
```yaml
changes:
  - description: "specific change to make"
    rationale: "why this improves code"
    files:
      - "affected files"
```

## Example
```yaml
changes:
  - description: "Extract length validation into separate function"
    rationale: "reduces validate_password complexity from 15 to 8 lines"
    files:
      - "src/auth/validation.rs"
  - description: "Add validation_result struct for error details"
    rationale: "enables callers to show specific validation errors"
    files:
      - "src/auth/validation.rs"
      - "src/api/handler.rs"
```
