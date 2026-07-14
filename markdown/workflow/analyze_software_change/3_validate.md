## Process
1. Scope checks — run both, accumulate all scope issues before outputting:
  - Coverage: map every functional and non-functional scope item to at least one plan step
  - Exclusion: verify no plan step introduces out-of-scope work
  - If any scope issues found → navigate to scope step
2. Flow check: verify steps are ordered so each builds on the previous. If issues found → navigate to plan step
3. If all checks pass, present scope and plan to the user for confirmation
  - On confirmation → end workflow
  - On scope issue → navigate to scope step
  - On plan issue → navigate to plan step

## Workflow Step Output
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

When user confirms: output nothing.
