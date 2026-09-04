## Purpose
Remove out-of-scope items from refactoring plan

## Process
1. Review each proposed change against constraints
2. Remove changes that:
   - Violate constraints
   - Are unrelated to user's requirement

## Choreo Move Artifact
```yaml
changes:
  - description: "specific change to make"
    rationale: "why this improves code"
    files:
      - "affected files"
removed:
  - description: "removed change"
    reason: "why it was removed"
```

## Example
```yaml
changes:
  - description: "Extract length validation into separate function"
    rationale: "reduces validate_password complexity"
    files:
      - "src/auth/validation.rs"
removed:
  - description: "Add validation_result struct for error details"
    reason: "violates constraint to preserve public interface"
```
