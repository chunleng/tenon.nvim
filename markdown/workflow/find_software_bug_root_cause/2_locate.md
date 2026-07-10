## Purpose
Find code causing bug

## Process

Trace execution path from reproduction steps:
- Identify entry points
- Follow execution flow
- No exception: Use a debugging unit test to show the divergence where expected ≠ actual

### Divergence Discovery
- Bug isolation: use debug print, log analysis, etc. to confirm unexpected behavior

## Output
```yaml
- file: "path/to/file"
  explanation: "how code causes bug (direct or indirect)"
```

## Example
```yaml
- file: "src/auth/login.rs"
  explanation: "validate_password() doesn't check empty string → crash on empty input"
- file: "src/api/handler.rs"
  explanation: "passes unvalidated user input to validate_password()"
```
