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
3. Report the review to chat, including only findings not marked `dropped`:
   - No such findings remain → ::tenon-chat:: "LGTM!"; end the choreo here.
   - Such findings remain → ::tenon-chat:: a one-line summary, then numbered findings, each with title and file path + line numbers, the problem/impact, and the fix (see "Output examples" section)
4. After reporting blockers, ::tenon-chat:: the request to address the findings by fixing, or to let you know if they decide not to fix
5. When the user responds, assign a decision to EVERY finding before navigating:
   - **resolved-via-code**: the user made code changes addressing this finding. A generic statement not naming a specific finding (e.g., "I've made the necessary code changes") → mark all outstanding findings as `resolved-via-code`
   - **dropped**: the user disagreed with reasoning you accept; drop the finding
   - **pending**: the user disagreed with reasoning you reject, or did not address this finding. If rejecting, ::tenon-chat:: the explanation of why you reject the reasoning
6. Only after every finding has a decision, determine the action (first match wins):
   - Any finding marked `resolved-via-code` → go to move 2 (re-review the updated diff)
   - All findings `dropped` with no code changes → ::tenon-chat:: "LGTM!"; end the choreo.
   - Any finding still `pending` → ::tenon-chat:: the prompt about the pending findings, asking again to fix or decline; then wait for the user's next response and repeat from process step 5

### Output examples

Blockers found:
```
<One-line summary>

## 1. **<Title>** (<path> L<lines>)
<Problem and impact>

Fix: <fix>
```

## Choreo Move Artifact
### If navigating to move 2 (code changes resolved)
```yaml
findings:
  - category: Correctness | Security | Clarity | Maintainability | Style
    title: Short description of the issue
    file: File path
    lines: Line number(s)
    decision: resolved-via-code | dropped | pending
```

### If navigating to end (all resolved or dropped, no code changes)
No artifact
