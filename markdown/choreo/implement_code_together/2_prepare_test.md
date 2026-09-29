## Process
Tests only, no code implementation.

If user mentions no test is needed for this goal or in general, and a directive condition permits skipping the test, navigate to next move without confirmation.

Determine if code behavior changes:
- No → follow "Behavior changes" section
- Yes → follow "Refactor changes" section

### Refactor changes
1. Search tests covering code to be refactored
   - Modify test so they pass after refactoring
2. Ensure test coverage
   - Code to be refactored not covered by any test → create test that passes after refactoring
3. Apply "Test Value Filter" to the candidate tests
4. Perform necessary edit on test files (kept tests only)
5. Verify tests
   - Run tests and capture status
   - Test may pass or fail, depending on test modification and refactoring type
6. Follow "Confirm with User" section

### Behavior changes
1. Search existing tests related to goal files/code
2. Identify candidate tests asserting goal behavior
   - Use "Decision: Modify vs. Create test" to create new test or append to existing
3. Apply "Test Value Filter" to the candidate tests
4. Perform necessary edit on test files (kept tests only)
5. Verify tests
   - Run test → when behavior changes, test must fail before code change
   - Test passes → ask: "Implemented already, or how to adjust test to capture the change?"
6. Follow "Confirm with User" section

### Test Value Filter
A goal may need multiple tests, but not all candidate tests are worth writing. Follow the Testing Basics directive when testing.

A designed test violates Testing Basics → question the test:
1. Is there a better way to design it? → redesign and re-check
2. Otherwise, is static verification (e.g. type check, build) enough to verify the change? → drop the test
3. Neither → keep the test

Dropped tests are dropped silently. Only kept tests are shown to the user.

No valuable test remains → navigate to next move without confirmation.

### Confirm with User
1. Show kept test targets
2. Say "Please confirm the test"
   - User confirmed, test created/modified or told to skip → next move
   - User requests changes → loop to process step 1

## Decision: Modify vs. Create test
**Cohesion test:** Would both verifications fail for the same reason?
- Yes → Same concept → Modify existing test
- No → Different concepts → Create separate test

**Example:**
- Changing password length requirement → Modify `test_password_validation` (fails for same reason: invalid length)
- Adding password complexity check → Create `test_password_complexity` (fails for different reason: missing special character, not length)

## Choreo Move Artifact
```yaml
tests:
  - test_file: path/to/test/file
    test_name: test function name
    status: failing|passing
    purpose: why this test is crucial for verifying the change
```
