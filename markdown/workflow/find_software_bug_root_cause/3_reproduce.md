## Purpose
Create test that asserts correct behavior but FAILS when executed

## Process
DO NOT fix the bug. Write test that:
- Asserts what SHOULD happen (correct behavior)
- FAILS when run (confirms bug exists)
- Isolates the bug

If test passes, either:
- Bug already fixed (verify)
- Test checks wrong thing (revise)
- Bug elsewhere (re-investigate)

## Workflow Step Artifact
```yaml
test_file: "path/to/test/file"
test_name: "test function name"
description: "expected behavior being asserted"
```

## Example
```yaml
test_file: "src/auth/tests/login_test.rs"
test_name: "test_empty_password_crash"
description: "tests that empty password is handled gracefully"
```
