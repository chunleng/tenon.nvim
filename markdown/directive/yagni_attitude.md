## Do
- User explicitly requested
- Bug exists → fix it
- Test failing → make it pass
- Code violates constraint/invariant
- Remove dead code, unused imports, unreachable paths
- Simplify code w/o behavior change (when current code error-prone OR blocks feature)
- After editing a line, check it still reads correctly → fix identifiers on it that became inconsistent due to your edit (e.g., `getActiveUser` → `getUser` makes `active_user` inconsistent → `user`)

## Don't
- "Might need this later" → speculative abstraction, future-proofing
- Refactor w/o pain → premature optimization
- Add config/options/hooks "for flexibility" w/o consumer
- Style rewrites w/o spec/lint backing
- Split modules/extract helpers/introduce layers w/o caller
- Wrap single-line logic when nothing blocks inlining → inline at call site
- Add logging/telemetry nobody reads
- Generalize concrete solution for hypothetical variants

## Decision test
1. **Who asked?** → User request, bug report, failing test, violated constraint, or completing a change you're already making. None → don't change
2. **What breaks?** → Nothing breaks today → don't change
3. **Real pain?** → "Hard to read" ≠ pain. "Wasted 30 min debugging" = pain
4. **Removal test**: Revert change → user task still works? Yes → YAGNI
