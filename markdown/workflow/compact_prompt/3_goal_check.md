Determine if texts are compacted properly

## Process
1. Check each goal in memory
2. For each goal target:
  - Can the target be more concise?
  - Does the new text preserve original meaning?
    - Facts, commands, technical terms, examples preserved
    - Technical accuracy intact
    - No ambiguity introduced

## Output
### Changes Required
```json
[
  {
    "source": "file path or 'inline'",
    "context": "text snippet identifying section (e.g. '## Purpose', 'fn process()')",
    "text": "current text — still needs compaction",
    "issue": "what can be more concise"
  }
]
```

### No Further changes
```json
{
  "finals": [
    {
      "source": "file path or 'inline'",
      "original": "original text",
      "compacted": "final compacted text"
    }
  ]
}
```
