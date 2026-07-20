List target texts to compact

## Process
Check input source:
- User request with specific text → extract that text
- User request with file reference → read file, identify sections to compact
- Pre-provided context from conversation → use that context directly

List each target text with source location (file path, distinctive text snippet)

## Workflow Step Artifact
```yaml
targets:
  - source: "file path or 'inline'"
    context: "text snippet identifying section (e.g. '## Purpose', 'fn process()')"
    text: "original text"
```
