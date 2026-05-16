## Purpose
Execute the planned incremental change

## Process
Write only code needed for test to pass:
- Avoid scope creep
- Ensure implementation actually fulfills the feature, not just passes the test

Follow project coding standards:
- Check AGENTS.md or project instructions
- Match existing code style
- Add comments only if necessary for understanding

Implementation guidelines:
- Don't add features not in plan
- Don't refactor unrelated code
- No abstractions unless required by test

## Output
```json
{
  "files_changed": ["path/to/file1", "path/to/file2"],
  "changes_made": "description of changes"
}
```

## Example
```json
{
  "files_changed": ["src/auth/validation.rs"],
  "changes_made": "Added empty string check at start of validate_password function"
}
```
