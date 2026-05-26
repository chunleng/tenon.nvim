## Purpose
Determine if texts are simplest form

## Process
1. Re-read target files for current state
2. For each compaction:
  a. Can text be simplified?
    - Remaining filler?
    - Symbols → replace more words?
    - All words necessary?
  b. Check type-specific:
    - Markdown → remove blank lines, unwrap paragraph breaks
    - Other types → apply type-specific rules
3. Simpler? → output all targets at once (not one by one). Simplest? → workflow ends

## Output

**targets list**

```json
{
  "targets": [
    {
      "source": "file path or 'inline'",
      "context": "text snippet identifying section (e.g. '## Purpose', 'fn process()')",
      "original": "current text",
      "simplified": "compacted version"
    }
  ]
}
```
