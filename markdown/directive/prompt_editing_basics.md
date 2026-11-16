Consider the edit against the full scope.
- Directive → all instructions within the directive
- Choreo → all instructions within the same move
- Tool → the tool's entire definition

## Decision Test
Run each clause in the scope against the test. First match wins.

- Is the clause a duplicate of another clause in the scope? → delete
- Does the clause say the same thing in other words (including negated twin like "fast and not slow") of another clause in the scope? → delete
- Does it state a reason that translates to no action? → delete
- Can the clause be interpreted in more than one way? → rewrite
- Does it state an action but leave unclear when to apply? → rewrite as "when {cause}, {effect}"
  - Does a legitimate exception break the action?
    - Example: "never delete files without asking" breaks when clearing your own scratch files → "when deleting files outside your working dir, ask the user"
  - Is a risky or complex action unguarded by a condition?
    - Example: "delete stale branches" is irreversible with no gate → "when a branch is merged and older than 30 days, delete it"
- Does the clause hedge its commitment (e.g. "try to", "ideally", "when possible")? → rewrite as imperative
- Does the prompt define the agent by role instead of desired characteristics (e.g. "You are a backend developer")? → rewrite, defining the characteristics
- Does the clause state its constraint after the content it constrains? → rewrite with the constraint first
- Is the desired behavior stated nowhere in the scope? → add
- Is the intent easier to show than describe? → add an example

## Technique for Clear Prompt

### Use Decision Test
Add a decision test (quick yes/no questions to apply it) only when the rule can't cover every case on its own.
- Example: Rule "don't over-engineer" → test: "Who asked? What breaks? Real pain?"

