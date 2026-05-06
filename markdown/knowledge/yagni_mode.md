## OK to change

- Bug exists → fix it
- User explicitly requested the change
- Test failing → make it pass
- Existing code violates stated constraint/invariant
- Removing dead code, unused imports, unreachable paths
- Simplifying code without altering behavior (only when current code is error-prone or blocks implementing a requested feature)

## Not OK to change

- "Might need this later" → speculative abstraction, future-proofing
- Refactor with no current pain point → premature optimization
- Adding config/options/hooks "for flexibility" with no current consumer
- Style/preference rewrites not backed by spec or lint rule
- Splitting modules, extracting helpers, introducing layers with no concrete current caller
- Adding logging/telemetry nobody is reading
- Generalizing a concrete solution to handle hypothetical variants
- Defensive code for states that cannot occur in current usage (e.g., nil check on value that is always non-nil today)

## Decision test

Before making a change, answer:

1. **Who asked?** → User request, bug report, failing test, or violated constraint. If none → don't change.
2. **What breaks if I don't?** → If nothing breaks today → don't change.
3. **Is the pain real?** → "Hard to read" is not pain. "I wasted 30 min debugging this" is pain.
4. **The removal test**: "If I revert this specific change, does the user's stated task still work correctly?" — If yes, the change is YAGNI.

If answer to (1) is "nobody" or answer to (2) is "nothing" → stop. Ship what exists.

## Edge cases

- Nil guard on value that can actually be nil → OK (present risk, not speculative)
- Nil guard on value that is never nil → Not OK (speculative defense)
- Adding type annotation to fix real type error → OK; cosmetic typing on untyped code → Not OK
- Extracting shared helper called from ≥2 sites → OK (real reuse)
- Extracting helper called from 1 site "for future reuse" → Not OK (speculative)
- Renaming after user confusion in PR/review → OK (pain is real and documented)
- Renaming because "the name could be better" → Not OK (no present pain)
