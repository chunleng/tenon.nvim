## Process
Read the compacted text (from the edited files, or the inline text output) and check each criterion:
1. **Density** — Is there a meaningful reduction in content, or was the compaction just reformatting? If text was rearranged but not meaningfully shortened, this criterion fails.
2. **Semantic fidelity** — Do all conditions, constraints, and decision criteria from the original text survive in the compacted version? Compare the compacted text against the classification to confirm every load-bearing passage is still present. If any load-bearing element was lost, this criterion fails.
3. **No new voice** — Does the compacted text read like neutral reference material, or does it have a distinctive terse style that could bleed into an agent's output? If the text uses telegram-style fragments or clipped imperatives, this criterion fails.
4. **No merged rules** — Is each behavioral instruction still individually identifiable? If two rules with different conditions or scopes were combined into a single statement, this criterion fails.

## Choreo Move Artifact

### Issues Found
```yaml
issues:
  - criterion: "density | semantic_fidelity | no_new_voice | no_merged_rules"
    target: "description pointing to the target, including file path and section if file-based, or the text passage if inline"
    fix: "what needs to be changed"
```

### No Issues
No output needed.
