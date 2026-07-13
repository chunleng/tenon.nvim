## Purpose
Execute the planned incremental change and verify it works

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

Verify after implementation:
- Build the project
- Run affected tests (test from Prepare Test step, tests in same module, or tests calling modified functions)
- If verification fails → fix issues and retry

## Workflow Step Output
```yaml
test_status: pass
```
