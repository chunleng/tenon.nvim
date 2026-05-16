## Purpose
Find next incremental step toward goal

## Process
Assess current state:
- What code exists now
- What's working vs broken
- What's already implemented

Identify gap between current state and requirements from Understand step (requirements persist throughout workflow):
- What's missing
- What needs changing
- Dependencies between changes

Find next incremental step:
- One logical unit of change (one function, one module, or one feature)
- Can be verified independently
- Moves toward goal
- Avoid large refactors (break into smaller steps)

## Output
```json
{
  "next_step": "description of the change",
  "files": ["path/to/file1", "path/to/file2"],
  "change_details": "specific modification to make",
  "verification": "how to verify this step works"
}
```

## Example
```json
{
  "next_step": "Add empty string check in validate_password function",
  "files": ["src/auth/validation.rs"],
  "change_details": "Add if password.is_empty() check at start of validate_password()",
  "verification": "test_empty_password_validation should pass after this change"
}
```
