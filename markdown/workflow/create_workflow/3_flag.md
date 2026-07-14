## Purpose
Review drafted workflow content and flag vague, ambiguous, or problematic lines

## Process
1. Read each step instruction file
2. For each line, assess:
   - Clarity: unambiguous?
   - Specificity: concrete or vague?
   - Redundancy: repeats unnecessarily?
   - Contradiction: conflicts with other instructions?
3. Flag lines needing attention with reason

## Flagging Criteria
- **Vague**: "be careful", "try to", "ideally" → needs specificity
- **Buried constraint**: Important rule buried in middle of paragraph
- **Redundant**: Same instruction repeated across steps
- **Contradictory**: Two instructions that conflict

## Workflow Step Output
```yaml
file: path/to/file.md
flagged_lines:
  - file: path/to/file.md
    line: 5
    content: ...
    reason: ...
  - file: path/to/file.md
    line: 12
    content: ...
    reason: ...
```

If no issues:
```yaml
flagged_lines: []
```
