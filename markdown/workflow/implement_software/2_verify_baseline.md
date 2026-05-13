## Purpose
Verify codebase is in good state before making changes

## Process
Run full test suite:
- Use project's test command (check AGENTS.md)
- Run all tests to establish baseline
- Document test results

If tests fail:
- Report all failures
- End workflow (cannot proceed from broken state)
- User must fix existing issues first

If tests pass:
- Proceed to Plan step
- Baseline verified, safe to proceed

## Output
```json
{
  "baseline_status": "pass|fail",
  "failed_tests": ["test_name_1: failure reason"],
  "error_message": "Cannot proceed from broken baseline"
}
```

## Example
```json
{
  "baseline_status": "pass"
}
```

```json
{
  "baseline_status": "fail",
  "failed_tests": [
    "test_auth: timeout error",
    "test_login: assertion failed"
  ],
  "error_message": "Cannot proceed from broken baseline. Fix existing test failures before making changes."
}
```
