## Purpose
Verify each edit preserves meaning

## Process
For each compaction:
1. Compare original vs compacted
2. Verify meaning unchanged:
   - Facts, commands, technical terms, examples preserved
   - Technical accuracy intact
   - No ambiguity introduced
3. Meaning changed? → revert to original

Output only verified compactions

## Output
```json
{
  "verified_compactions": [
    {
      "source": "file path or 'inline'",
      "context": "text snippet identifying section (e.g. '## Purpose', 'fn process()')",
      "original": "original text",
      "compacted": "compacted text"
    }
  ]
}
```
