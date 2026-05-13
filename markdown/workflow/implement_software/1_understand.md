## Purpose
Gather requirements and define verification criteria

**Important**: Requirements persist throughout the entire workflow. All subsequent steps reference these requirements.

## Process
Check input source:
- User request: extract requirements
- Previous workflow output: use as requirements

**If requirements unclear → ask user for clarification before proceeding**

Define acceptance criteria:
- What must be true when implementation complete
- Measurable outcomes

Define verification methods:
- How to verify each criterion
- Tests (preferred), or build + analysis together

## Output
```json
{
  "requirements": "clear statement of what needs to be implemented",
  "acceptance_criteria": [
    {
      "criterion": "what must be true",
      "verification": {
        "method": "test|build+analysis",
        "details": "specific test name or build command"
      }
    }
  ]
}
```

## Example
```json
{
  "requirements": "Add empty string validation to password input",
  "acceptance_criteria": [
    {
      "criterion": "Empty password returns validation error",
      "verification": {
        "method": "test",
        "details": "test_empty_password_validation in auth_test.rs"
      }
    },
    {
      "criterion": "Non-empty password proceeds to authentication",
      "verification": {
        "method": "test",
        "details": "test_nonempty_password_proceeds in auth_test.rs"
      }
    }
  ]
}
```
