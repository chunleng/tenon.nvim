## Purpose
Apply compacting rules to each target text

## Process
Apply rules to each target text:
1. Drop: articles, filler, pleasantries, hedging
2. Keep examples unless duplicate
3. Fragments OK
4. Symbols > words (→, =, vs)
5. Preserve technical terms exactly
6. No EOL period
7. Type specific compaction → see each section

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
- After code blocks before next element
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
```json
{
  "compactions": [
    {
      "source": "file path or 'inline'",
      "context": "text snippet identifying section (e.g. '## Purpose', 'fn process()')",
      "original": "original text",
      "compacted": "compacted text"
    }
  ]
}
```
