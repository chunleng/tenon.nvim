## OK to change

- Bug exists → fix it
- User explicitly requested
- Test failing → make it pass
- Code violates constraint/invariant
- Remove dead code, unused imports, unreachable paths
- Simplify code w/o behavior change (current code error-prone OR blocks feature)

## Not OK to change

- "Might need this later" → speculative abstraction, future-proofing
- Refactor w/o pain → premature optimization
- Add config/options/hooks "for flexibility" w/o consumer
- Style rewrites w/o spec/lint backing
- Split modules/extract helpers/introduce layers w/o caller
- Add logging/telemetry nobody reads
- Generalize concrete solution for hypothetical variants
- Defensive code for impossible states (e.g., nil check on always-non-nil)

## Decision test

Before change, answer:

1. **Who asked?** → User request, bug report, failing test, violated constraint. None → don't change
2. **What breaks?** → Nothing breaks today → don't change
3. **Real pain?** → "Hard to read" ≠ pain. "Wasted 30 min debugging" = pain
4. **Removal test**: Revert change → user task still works? Yes → YAGNI

(1) = "nobody" OR (2) = "nothing" → stop. Ship what exists

## Edge cases

- Nil guard on nilable value → OK (present risk)
- Nil guard on never-nil value → Not OK (speculative)
- Type annotation fixing real error → OK; cosmetic typing on untyped → Not OK
- Extract helper called from ≥2 sites → OK (real reuse)
- Extract helper called from 1 site "for future reuse" → Not OK
- Rename after user confusion in PR/review → OK (documented pain)
- Rename because "could be better" → Not OK (no pain)
