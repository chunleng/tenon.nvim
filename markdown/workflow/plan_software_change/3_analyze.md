## Purpose
Identify implementation changes: new components to create, existing code to modify/delete

## Process
1. If this is a revisit (issues returned from Verify Plan step), address the issues first
2. Identify files and components involved:
   - Search codebase for keywords related to requirement
   - Trace data flow from user input to output
   - Check architecture docs for component boundaries
   - Identify entry points (APIs, handlers, main functions)
3. Identify integration points:
   - Where new/changed components connect to existing code
   - APIs that need modification
   - Data structures that need changes

## Output
```json
{
  "changes": [
    {
      "target": "file or component",
      "description": "what to do",
      "rationale": "why this change",
      "integration_points": ["where it connects"]
    }
  ]
}
```

## Example
```json
{
  "changes": [
    {
      "target": "src/rate_limiter.rs",
      "description": "create token bucket rate limiter",
      "rationale": "encapsulates rate limiting logic",
      "integration_points": ["middleware layer"]
    },
    {
      "target": "src/middleware.rs",
      "description": "modify to add rate limiter to request pipeline",
      "rationale": "applies rate limiting to all requests",
      "integration_points": ["request handler", "rate_limiter"]
    }
  ]
}
```
