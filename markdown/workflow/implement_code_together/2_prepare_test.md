## Purpose
Ensure test verifies incremental change. Tests only — no implementation

## Process
Determine change type from Listen step

> **Note**: Collaborating w/ user. May have future code. Don't remove w/o confirming. Blocked → ask user

### Refactor changes (behavior preserved)
1. Identify affected tests
   - Search for tests covering refactored code
   - List all tests exercising affected logic
2. Ensure test coverage
   - If test exists → document as verification point
   - If no test → create passing test for refactored logic
   - Use cohesion test if unsure whether to modify or create
3. Verify tests pass
   - Run tests to confirm pass before refactoring
   - Tests verify behavior remains correct after refactor

### Code changes (new behavior)
1. Search for existing tests
   - Find tests related to files/code in goal
   - Identify tests that can be modified
2. Prepare test
   - If no existing test → create new test file if needed, write test asserting behavior in goal
   - If existing test found → use cohesion test to decide modify vs. create
3. Verify test fails
   - Run test to confirm failure
   - Failing test = change not yet made
   - Passing test = goal already implemented or test incorrect → ask user: "Test passing. Implemented already, or should test be adjusted?"

### Present & Confirm
1. Output tests table to chat directly
   - Exact columns: Function Name, File, New/Existing, Test Run Status
2. Confirm w/ user
   - Confirm → proceed to Implement
   - Changes requested → adjust & re-confirm

## Decision: Modify vs. Create test
**Cohesion test:** Would both verifications fail for same reason?
- Yes → Same concept → Modify existing test
- No → Different concepts → Create separate test

**Example:**
- Changing password length requirement → Modify `test_password_validation` (fails for same reason: invalid length)
- Adding password complexity check → Create `test_password_complexity` (fails for different reason: missing special character, not length)

## Output
```json
[
  {
    "test_file": "path/to/test/file",
    "test_name": "test function name",
    "status": "failing|passing",
    "purpose": "why this test is crucial for verifying the change"
  }
]
```

## Example
**Code change:**
```json
[
  {
    "test_file": "src/auth/tests/validation_test.rs",
    "test_name": "test_empty_password_validation",
    "status": "failing",
    "purpose": "Confirms password validation rejects empty strings"
  }
]
```

**Refactor change:**
```json
[
  {
    "test_file": "src/utils/parser_test.rs",
    "test_name": "test_parse_config",
    "status": "passing",
    "purpose": "Ensures config parsing logic remains correct after extracting parse logic into separate module"
  },
  {
    "test_file": "src/utils/parser_test.rs",
    "test_name": "test_parse_edge_cases",
    "status": "passing",
    "purpose": "Covers edge cases that must remain working after refactoring"
  }
]
```
