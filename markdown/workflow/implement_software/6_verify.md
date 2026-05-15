## Purpose
Verify affected tests pass

## Process
Run build:
- Skip if build already run in previous step
- Use project's build command (check AGENTS.md)
- If build fails → return to Implement step

Run affected tests:
- Skip if tests already run in previous step
- Run test from Prepare Test step, tests in same module, or tests calling modified functions
- Run targeted tests only, don't run full test suite
- If test fails → return to Implement step
- If test passes → proceed to Goal Check

## Output
```json
{
  "build_status": "pass|fail",
  "tests_run": ["test_name_1", "test_name_2"],
  "test_status": "pass|fail",
  "failed_tests": ["test_name_1: failure reason"]
}
```

## Example
```json
{
  "build_status": "pass",
  "tests_run": ["test_empty_password_validation"],
  "test_status": "pass"
}
```

```json
{
  "build_status": "pass",
  "tests_run": ["test_empty_password_validation", "test_password_hash"],
  "test_status": "fail",
  "failed_tests": ["test_password_hash: assertion failed at line 45"]
}
```
