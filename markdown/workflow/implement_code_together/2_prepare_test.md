## Purpose
Ensure test verifies incremental change. Tests only — no implementation

## Process
> **Note**: Collaborating w/ user. May have future code. Don't remove w/o confirming. Blocked → ask user

1. Understand user intent & determine change type
   - Review goal, clarify if needed
   - Determine: behavior preserved?
     - Yes → refer+execute "Refactor changes" section
     - No → refer+execute "Code changes" section
   - Continue below after the tests are updated
2. Output tests table
   - Columns: Function Name, File, New/Existing, Test Run Status
3. **Confirm w/ user**
   - Confirmed + test written or explicitly told to skip → proceed to next workflow step
   - Changes → loop back to understand user intent

## Refactor changes (behavior preserved)
1. Identify affected tests
   - Search tests covering refactored code
   - List all tests exercising affected logic
2. Ensure test coverage
   - Existing test → document as verification point
   - No test → create passing test for refactored logic
   - Use cohesion test if unsure: modify or create
3. Verify tests pass
   - Run tests before refactoring
   - Confirm behavior remains correct

## Code changes (new behavior)
1. Search existing tests
   - Find tests related to goal files/code
   - Identify tests that can be modified
2. Prepare test
   - No existing test → create new file, write test asserting goal behavior
   - Existing test → use cohesion test: modify vs create
3. Verify test fails
   - Run test → confirm failure
   - Failing = change not yet made
   - Passing = goal implemented or test incorrect → ask: "Test passing. Implemented already, or should test be adjusted?"

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
