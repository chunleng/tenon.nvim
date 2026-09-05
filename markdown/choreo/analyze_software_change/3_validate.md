## Process
1. Scope checks — run both, accumulate all scope issues before providing:
  - Coverage: map every functional and non-functional scope item to at least one plan step
  - Exclusion: verify no plan step introduces out-of-scope work
  - If any scope issues found → navigate to scope move
2. Setup checks — accumulate all setup issues before providing:
  - Justification: every setup step names a concrete feature or non-functional requirement that requires it
  - Just-in-time: every setup step is placed immediately before the first feature step that needs it, not batched upfront
  - If any setup issues found → navigate to plan move
3. Flow check: verify steps are ordered so each builds on the previous. If issues found → navigate to plan move
4. If all checks pass, present scope and plan to the user for confirmation
  - On confirmation → end choreo
  - On scope changed/issue → navigate to scope move
  - On plan changed/issue → navigate to plan move

## Choreo Move Artifact
When scope issues are found:
```yaml
scope_issue:
  - description of the first scope issue
  - ...
```

When plan issues are found:
```yaml
plan_issue:
  - description of the first plan issue
  - ...
```

When user confirms: provide nothing.
