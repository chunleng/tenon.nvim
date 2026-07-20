## Purpose
Remove debugging artifacts and conclude root cause analysis with recommendations

## Process
1. Remove debugging artifacts added during investigation:
   - Debug prints
   - Temporary logs
2. Identify root cause from code inspection + test result
3. Check if surrounding logic is overly complex

## Workflow Step Artifact
```yaml
cause: "root cause write-up"
recommendation: "test created (if any), fix suggestion, refactoring suggestion if code is complex"
```

## Example
```yaml
cause: "validate_password() doesn't check empty string, causing crash when user submits empty password"
recommendation: "test created: test_empty_password_crash. Fix: add empty string check in validate_password(). Consider: validation logic is scattered across 3 files - refactor into single validation module"
```
