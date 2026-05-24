## Purpose
Full verification and documentation

## Process
Run full verification:
- Batch tool calls when running multiple commands
- Build entire project
- Run full test suite
- Check project standards (lint, format, etc.)

Document unverifiable aspects:
- Visual rendering (browser, UI)
- User experience
- Visual layout, spacing, alignment
- Load time, responsiveness, memory usage

Ask user to verify unverifiable aspects:
- List aspects agent cannot verify
- Request user confirmation or testing
- Document user feedback

Final output:
- All verifications passed
- Unverifiable aspects documented
- User confirmation obtained

## Output
```json
{
  "build_status": "pass|fail",
  "test_status": "pass|fail",
  "project_standards": "pass|fail|not applicable",
  "unverifiable_aspects": [
    {
      "aspect": "description",
      "reason": "why agent cannot verify"
    }
  ],
  "failed_tests": [
    "file:test: message"
  ],
  "user_verification": "confirmed|pending|failed"
}
```

## Example
```json
{
  "build_status": "pass",
  "test_status": "pass",
  "project_standards": "pass",
  "unverifiable_aspects": [
    {
      "aspect": "UI rendering of error message",
      "reason": "Agent cannot see browser rendering"
    }
  ],
  "user_verification": "confirmed"
}
```

```json
{
  "build_status": "pass",
  "test_status": "fail",
  "project_standards": "pass",
  "unverifiable_aspects": [],
  "failed_tests": [
    "src/auth/test_auth: test_auth failed: timeout",
    "src/auth/test_login: test_login failed: wrong error code"
  ],
  "user_verification": "pending"
}
```
