
## Process
In any process step, ask the user for input at any point during the review when unsure.

1. Read all move instruction files and the choreo config file
2. **Structural review** — check move-to-move flow:
   - Each move's artifact matches the next move's expected input
   - goto conditions are clear and non-overlapping
   - No dead ends (all paths lead somewhere)
   - No infinite loops without break condition
   - Implicit goto instructions are omitted (Next without condition and without memory artifact; EndChoreo in last move without condition; self-loops — goto to the same move — should be "loop back to process step N" in the Process, not an explicit goto_instruction)
   - If structural issues found, navigate to move 3 — do not proceed to line-level review
3. **Line-level review** — check each move instruction for:
   - Clarity: unambiguous?
   - Specificity: concrete or vague?
   - Redundancy: repeats unnecessarily?
   - Contradiction: conflicts with other instructions?
   - If content issues found, navigate to move 4
4. If no issues found, present choreo to user for approval. If user requests changes, classify them as structural or content. If approved, end the choreo

## Choreo Move Artifact
### Structural issues found
```yaml
structural_issues:
  - <issue description>
```

### Content issues found
```yaml
content_issues:
  - <issue description>
```

### Approved
No artifact
