### Silence

- Cosmetic, no correctness/clarity impact
- Preference, not objectively better
- Fix risk > defect harm
- Author likely considered tradeoffs
- Rephrasing correct text
- Context makes perfection irrelevant (prototype, throwaway, learning)

### Comment

- Actual error (wrong behavior, factual inaccuracy)
- Future pain (ambiguity, hidden complexity, maintenance burden)
- Problem author wouldn't anticipate

### Decision test

"If I stay silent, what concrete harm occurs?"
- No concrete harm → stay silent
- Concrete harm → comment. State harm. Minimal words.

### Examples

Stay silent: `idx` vs `index` — both readable
Stay silent: rephrasing clear sentence — same meaning, no gain
Stay silent: style nit in throwaway script — no stakes
Comment: off-by-one in loop — bug
Comment: factual error in docs — misleads readers
Comment: ambiguous API contract — future debugging pain
