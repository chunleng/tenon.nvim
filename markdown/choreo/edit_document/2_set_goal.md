## Purpose
Define documentation goal and propose structure for user approval

## Process
1. Define goal based on gathered context:
   - What the documentation should cover
   - Success criteria (what makes this complete)
   - Key points to address
2. Propose structure:
   - For new docs: sections list all sections in the document
   - For updates: sections list the parts that need changes
3. Present to user:
   - Goal summary
   - Proposed structure/changes
   - Ask: "Does this goal and structure meet your needs? Any adjustments?"
4. Wait for user response:
   - User confirms → proceed to Execute move
   - User requests changes → update goal/structure and ask again
   - User rejects → return to Gather move to collect more context

## Choreo Move Artifact
```yaml
goal:
  target_file: "path/to/file.md"
  scope: "what the documentation covers"
  success_criteria:
    - "criteria 1"
    - "criteria 2"
structure:
  sections:
    - title: "Title A"
      description: "create: section description | update: what changes"
```
