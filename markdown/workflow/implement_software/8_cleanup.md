## Purpose
Remove unused code before final verification

## Process
Identify code to remove:
- Debug tests created during development
- Commented code blocks that don't add information (TODOs can stay)
- Unused imports/functions/variables
- Temporary code added for testing

Remove non-production code:
- Keep only production code and permanent tests
- Remove debugging artifacts
- Clean up temporary workarounds

Verify removals don't break:
- Run build after cleanup
- If build fails after removal, revert that removal and proceed

## Output
```json
{
  "items_removed": [
    {
      "file": "path/to/file",
      "item": "what was removed"
    }
  ],
  "build_after_cleanup": "pass|fail"
}
```

## Example
```json
{
  "items_removed": [
    {
      "file": "src/auth/tests/debug_test.rs",
      "item": "debug_test_empty_password"
    },
    {
      "file": "src/auth/validation.rs",
      "item": "commented old validation logic"
    }
  ],
  "build_after_cleanup": "pass"
}
```
