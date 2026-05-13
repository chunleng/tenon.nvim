## Purpose
Ensure test exists that verifies the incremental change

## Process
Check existing tests:
- Search for tests matching verification criteria
- Identify tests that can be modified

If no existing test:
- Create new test file if needed
- Write test asserting expected behavior
- Test must FAIL (confirms change not yet implemented)

If existing test matches:
- Verify it matches verification criteria from Understand step
- Modify if needed to match criteria
- Verify it FAILS before implementation

Run test to confirm failure:
- Failing test = change not yet made
- Passing test = change already done or test is wrong

## Output
```json
{
  "test_file": "path/to/test/file",
  "test_name": "test function name",
  "action": "created|modified|found",
  "status": "failing",
  "expected_behavior": "what the test asserts"
}
```

## Example
```json
{
  "test_file": "src/auth/tests/validation_test.rs",
  "test_name": "test_empty_password_validation",
  "action": "created",
  "status": "failing",
  "expected_behavior": "asserts validate_password returns error on empty string"
}
```
