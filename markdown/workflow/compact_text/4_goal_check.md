## Purpose
Determine if texts are simplest form

## Process
For each verified compaction:
1. Check if text can be further simplified:
   - Any remaining filler?
   - Can symbols replace more words?
   - Are all words necessary?
2. Simpler? → output targets. Simplest? → workflow ends

## Output

**targets list**

```json
{
  "targets": [
    {
      "source": "file path or 'inline'",
      "context": "text snippet identifying section (e.g. '## Purpose', 'fn process()')",
      "text": "text to change"
    }
  ]
}
```
