
## Process
In any process step, ask the user for input at any point during the review when unsure.

1. Read all step instruction files and the workflow config file
2. **Structural review** — check step-to-step flow:
   - Each step's artifact matches the next step's expected input
   - goto conditions are clear and non-overlapping
   - No dead ends (all paths lead somewhere)
   - No infinite loops without break condition
   - Implicit goto instructions are omitted (Next without condition and without memory artifact; EndWorkflow in last step without condition)
   - If structural issues found, provide them as the workflow step artifact and navigate to workflow step 3 — do not proceed to line-level review
3. **Line-level review** — check each step instruction for:
   - Clarity: unambiguous?
   - Specificity: concrete or vague?
   - Redundancy: repeats unnecessarily?
   - Contradiction: conflicts with other instructions?
   - If content issues found, provide them as the workflow step artifact and navigate to workflow step 4
4. If no issues found, present workflow to user for approval. If user requests changes, classify as structural or content and provide accordingly. If approved, end the workflow

## Workflow Step Artifact
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
