## Purpose
Ensure test exists that verifies the incremental change

## Process
Determine change type from Plan step

### Refactor changes (behavior preserved)

1. Identify affected tests
   - Search for tests covering the code being refactored
   - List all tests that exercise the affected logic

2. Ensure test coverage
   - If test exists: Document it as verification point
   - If no test: Create passing test for the logic being refactored

3. Verify tests pass
   - Run tests to confirm they pass before refactoring
   - These tests will verify behavior remains correct after refactor

### Code changes (new behavior)

1. Search for existing tests
   - Find tests matching verification criteria
   - Identify tests that can be modified

2. Prepare test
   - If no existing test: Create new test file if needed, write test asserting expected behavior
   - If existing test matches: Modify to match verification criteria from Understand step

3. Verify test fails
   - Run test to confirm failure
   - Failing test = change not yet made
   - Passing test = change already done or test is wrong

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
