## Purpose
Understand user's refactoring requirement and define constraints

## Process
1. Analyze user's request to understand what they want to refactor and why
2. Investigate codebase to understand context:
  - What does the target code do
  - Who uses it (callers, dependents)
  - What interfaces does it expose
3. Ask clarifying questions if anything unclear:
  - What problem are you trying to solve
  - What must stay the same (behaviors, APIs, performance)
  - What can change
  - Any deadlines or constraints
4. Iterate: investigate → ask questions → refine understanding until constraints are clear

## Output
```yaml
requirement: "what user wants to achieve"
constraints:
  - "must preserve X behavior"
  - "cannot change Y interface"
  - "must maintain Z performance characteristic"
context: "relevant information discovered during investigation"
```

## Example
```yaml
requirement: "refactor login module to make it easier to add new authentication methods"
constraints:
  - "public API must remain unchanged"
  - "all existing tests must pass"
  - "login flow behavior must be identical"
  - "no performance regression"
context: "login.rs has 3 auth methods (password, oauth, sso). Team wants to add 2 more methods next quarter."
```
