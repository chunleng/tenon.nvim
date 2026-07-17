## Process
1. Think hard about how the implementation could fail
  a. What inputs fall outside the normal path?
  b. What assumptions does the implementation rely on?
  c. What existing behavior might this change have disrupted?
2. Verify based on the weaknesses identified in process step 1
  a. Prefer writing tests — a test that passes no matter what the code does proves nothing, so target the failure modes
  b. If untestable, run the build/lint/type-check and reason about whether the output covers the weaknesses
  c. If neither applies, mark it as unverifiable
3. Run new verification plus existing checks in the areas you touched

## Workflow Step Output

### If verification failed
```yaml
failures:
  - "what broke and why"
```
Routes back to Implement.

### If verification passed
```yaml
unverifiable:
  - "what couldn't be verified automatically and why"
```
Leave empty if nothing was unverifiable. Do not invent items. Accumulate with items already in workflow memory.
