## Purpose
Conclude root cause analysis with recommendations

## Process
- Identify root cause from code inspection + test result
- Check if surrounding logic is overly complex

## Output
```yaml
cause: "root cause write-up"
recommendation: "test created (if any), fix suggestion, refactoring suggestion if code is complex"
```

## Example
```yaml
cause: "validate_password() doesn't check empty string, causing crash when user submits empty password"
recommendation: "test created: test_empty_password_crash. Fix: add empty string check in validate_password(). Consider: validation logic is scattered across 3 files - refactor into single validation module"
```
