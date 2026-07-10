Apply compacting rules to each target text

## Process
Apply rules to each target text:
1. If returning from goal check step, resolve the issues
2. Express target in concise words
  - Ensure meaning is preserved
  - Don't drop subject/object unless proven redundant
  - Don't drop examples unless identical
3. Preserve similar structure, wherever possible
4. Type specific compaction → see each section

Edit target file. Report edits for next step

## Compacting Markdown
- Remove unnecessary newlines while preserving semantic separation.
- Unwrap paragraph and list line breaks.

### Keep blank lines between:
- Paragraphs
- Paragraphs and headers (before header)
- Between consecutive headers
- Lists and intervening text
- Unrelated structural elements

### Remove blank lines:
- After headers
- Between consecutive list items
- Around nested list markers

### Example:
```
Before:
# Section

## Subsection
text

- a

- b

After:
# Section

## Subsection
text
- a
- b
```

## Output
```yaml
compactions:
  - source: "file path or 'inline'"
    context: "text snippet identifying section (e.g. '## Purpose', 'fn process()')"
    original: "original text"
    compacted: "compacted text"
```
