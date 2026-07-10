## Purpose
Remove out-of-scope or unnecessary changes

## Process
1. Review each proposed change against constraints
2. Remove changes that:
   - Violate constraints
   - Are unrelated to user's requirement
   - Add unrequested functionality
3. Document rationale for each removal

## Output
```yaml
changes:
  - target: "file or component"
    description: "what to do"
    rationale: "why this change"
removed:
  - change: "removed change description"
    reason: "why removed"
```

## Example
```yaml
changes:
  - target: "src/rate_limiter.rs"
    description: "create token bucket rate limiter"
    rationale: "encapsulates rate limiting logic"
removed:
  - change: "add rate limit configuration UI"
    reason: "not in scope - requirement only mentions backend rate limiting"
```
