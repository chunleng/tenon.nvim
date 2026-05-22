## Purpose
Apply compacting rules to each target text

## Process
For each target text, apply rules:
1. Drop: articles, filler, pleasantries, hedging
2. Keep examples unless duplicate
3. Fragments OK
4. Symbols > words (→, =, vs)
5. Preserve technical terms exactly
6. No EOL period

Edit target file directly. Report edits for next step

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
