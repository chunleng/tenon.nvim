## Process

1. Check the input source:
   - User request with specific text → use that text
   - User request with file reference → read the file, identify the sections to compact
2. If the target is unclear, ask the user to clarify
3. Write a free-text description of what to compact. If the target is file-based, include file paths and which sections are in scope. If the target is inline text, include the text itself.

## Workflow Step Artifact
```yaml
goal: |
  Free-text description of what to compact. If file-based, include file paths and sections for later steps to locate the target text. If inline text, include the text itself.
```
