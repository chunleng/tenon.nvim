## Purpose
Confirm requirements are complete, unambiguous, and non-contradictory with existing system

## Process
1. If this is a revisit (issues returned from Elicit step), address the issues first
2. Check completeness:
   - Each acceptance criterion has clear verification method
   - All user-facing behaviors are specified
   - Edge cases are identified
3. Check clarity:
   - No vague terms without definitions
   - No ambiguous requirements (multiple interpretations possible)
   - Technical terms are well-defined
4. Check consistency:
   - Requirements don't contradict existing system behavior
   - Constraints don't conflict with each other
   - Acceptance criteria are mutually compatible

## Workflow Step Output
When verification fails:
```yaml
status: "verification failed"
issues:
  - issue: "description of problem"
    type: "incomplete | ambiguous | contradictory"
    suggestion: "how to resolve"
```

When verification passes: output nothing
