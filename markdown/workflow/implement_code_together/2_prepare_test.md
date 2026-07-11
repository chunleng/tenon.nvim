## Process
Determine if code behavior changes:
- No → follow "Behavior changes" section
- Yes → follow "Refactor changes" section

### Refactor changes
1. Search tests covering code to be refactored
   - Modify test so they pass after refactoring
2. Ensure test coverage
   - Code to be refactored not covered by any test → create test that passes after refactoring
3. Perform necessary edit on test files
4. Verify tests
   - Run tests and capture status
   - Test may pass or fail, depending on test modification and refactoring type
5. Follow "Confirm with User" section

### Behavior changes
1. Search existing tests related to goal files/code
2. Write test asserting goal behavior
   - Use "Decision: Modify vs. Create test" to create new test or append to existing
3. Perform necessary edit on test files
4. Verify tests
   - Run test → when behavior changes, test must fail before code change
   - Test passes → ask: "Implemented already, or how to adjust test to capture the change?"
5. Follow "Confirm with User" section

### Confirm with User
1. Show tests targets
2. Say "Please confirm the test"
   - User confirmed, test created/modified or told to skip → next workflow step
   - User requests changes → loop to process step 1

## Decision: Modify vs. Create test
**Cohesion test:** Would both verifications fail for the same reason?
- Yes → Same concept → Modify existing test
- No → Different concepts → Create separate test

**Example:**
- Changing password length requirement → Modify `test_password_validation` (fails for same reason: invalid length)
- Adding password complexity check → Create `test_password_complexity` (fails for different reason: missing special character, not length)

## Workflow Step Output
```yaml
- test_file: path/to/test/file
  test_name: test function name
  status: failing|passing
  purpose: why this test is crucial for verifying the change
```

## Example
**Code change:**
```yaml
- test_file: src/auth/tests/validation_test.rs
  test_name: test_empty_password_validation
  status: failing
  purpose: Confirms password validation rejects empty strings
```

**Refactor change:**
```yaml
- test_file: src/utils/parser_test.rs
  test_name: test_parse_config
  status: passing
  purpose: Ensures config parsing stays correct after extracting parse logic to separate module
- test_file: src/utils/parser_test.rs
  test_name: test_parse_edge_cases
  status: passing
  purpose: Covers edge cases that must work after refactoring
```
