## Process

### First visit (no existing plan in memory)
1. Read the goal and the relevant parts of the codebase
2. Break the work into discrete changes
  a. Small enough to implement and verify on its own
  b. Describe what to change — not which specific files or how to verify
  c. Ordered so earlier changes don't get invalidated by later ones
3. Pick the first change to make

### Re-visits (plan exists in memory)
1. Cross off what's done
2. Adjust what remains based on what you learned from the last implementation and verification
  a. Remove steps that turned out to be unnecessary
  b. Add steps for issues discovered during verification
  c. Re-order if dependencies turned out different than expected
3. Pick the next change to make

## Workflow Step Artifact
```yaml
done:
  - "what is done"
remaining:
  - "what to do"
next: "what is next"
```
