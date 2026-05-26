## Purpose
Ensure test exists verifying incremental change

## Process
Determine change type from Plan step

### Refactor changes (behavior preserved)
1. Identify affected tests
   - Search for tests covering refactored code
   - List all tests exercising affected logic
2. Ensure test coverage
   - If test exists: Document as verification point
   - If no test: Create passing test for logic being refactored
   - Use cohesion test if unsure whether to modify or create
3. Verify tests pass
   - Run tests to confirm they pass before refactoring
   - These tests verify behavior remains correct after refactor

### Code changes (new behavior)
1. Search for existing tests
   - Find tests matching verification criteria
   - Identify tests that can be modified
2. Prepare test
   - If no existing test: Create new test file if needed, write test asserting expected behavior
   - If existing test found: Use cohesion test to decide modify vs. create
3. Verify test fails
   - Run test to confirm failure
   - Failing test = change not yet made
   - Passing test = change already done or test is wrong

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
**Code change example:**
```json
[
  {
    "test_file": "src/auth/tests/validation_test.rs",
    "test_name": "test_empty_password_validation",
    "status": "failing",
    "purpose": "Confirms password validation rejects empty strings, which is the new behavior being added"
  }
]
```

**Refactor change example:**
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
