## Process
1. Determine the diff source from the user's request:
   - **Local git** → use git diff
   - **Remote platform** → use available tools to fetch the diff
2. Identify the base reference for comparison (omit for remote platform PRs — the diff is self-contained)
3. If the source or base is ambiguous or missing, ask the user how to obtain the diff
4. The user can override the range or target to review specific changes

## Workflow Step Artifact
The diff source configuration:
```yaml
source: How to obtain the diff (e.g., git diff against main, fetch from PR #123)
base: The base reference for comparison (omit for remote platform PRs — the diff is self-contained)
```
