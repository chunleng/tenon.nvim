## Process
1. Go through each acceptance criterion from the goal
  a. Is it actually met; verified, not just implemented?
  b. For each unmet criterion, check if it is no further work: it was marked unverifiable during Verify (see choreo memory), or its verification depends on such a criterion, directly or transitively
2. If every criterion is met or no further work, you're done
3. If unmet criteria remain that have further work, figure out exactly what the remaining gap is

## Choreo Move Artifact

### If every criterion is met or no further work
No artifact needed.

### If unmet criteria remain that have further work
```yaml
remaining_gap:
  - what's still needed
```
