## Process
1. Determine the context of each changed area:
   - Infer from file paths (e.g., files under `/test` → test code, `/src/db` → data layer)
   - Trace code content when the path alone is ambiguous
   - Classify each changed area independently
2. Apply filtering rules:
   - Drop compilation errors
   - Drop linter warnings
   - Drop findings about unchanged code
   - Filter by context (first match wins):
     | Context | Keep |
     |---------|------|
     | Test code | Correctness only |
     | Prototype/internal tool | Lenient, drop minor issues |
     | Frontend client | UX-blocking issues |
     | Core library | All, strict |
     | Data layer | Correctness + security |
     | Scripts/utilities | Correctness only |
3. Output the review to chat:
   - No blockers → Output exactly `LGTM!`
   - Blockers found → Output a one-line summary, then numbered findings, each with title and file path + line numbers, the problem/impact, and the fix (See "Output examples" section)
4. After reporting, handle the user's response:
   - If the user indicates they have made code changes → go to workflow step 2 (re-gather and re-review the updated diff)
   - If the user disagrees with a finding by providing reasoning → evaluate the reasoning:
     - Accept the reasoning → drop the finding, update the review
     - Maintain the finding → explain why
   - If all findings are dropped through accepted reasoning → output `LGTM!`
   - Continue discussing until the user fixes code or all findings are resolved

### Output examples

No blockers:
```
LGTM!
```

Blockers found:
```
<One-line summary of main concern>

## 1. **Race condition in user fetch** (src/api/user.rs L45-52)

Concurrent requests can overwrite `self.cache` without synchronization. Causes stale data on rapid navigation.

Fix: Wrap in `Mutex` or use `DashMap` for concurrent access.
```
