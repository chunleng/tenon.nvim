## Purpose
Gather requirements + define verification criteria

**Important**: Requirements persist throughout workflow → all subsequent steps reference

## Process
**If requirements unclear → ask clarification before proceeding**

**If test framework not found → ask user:**
- Setup test framework?
- Improves verification

Define acceptance criteria:
- What must be true when done
- Measurable outcomes

Define verification methods:
- How to verify each criterion
- Tests (preferred)
- Build + analysis: Untestable (language/visual limits). Last resort.

## Workflow Step Output
```yaml
requirements: "clear statement of what needs to be implemented"
acceptance_criteria:
  - criterion: "what must be true"
    verification:
      method: "test|build+analysis"
      details: "specific test name or build command"
```

## Example
```yaml
requirements: "Add empty string validation to password input"
acceptance_criteria:
  - criterion: "Empty password returns validation error"
    verification:
      method: "test"
      details: "test_empty_password_validation in auth_test.rs"
  - criterion: "Non-empty password proceeds to authentication"
    verification:
      method: "test"
      details: "test_nonempty_password_proceeds in auth_test.rs"
```
