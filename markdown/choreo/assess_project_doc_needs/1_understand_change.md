## Process
1. If the user has not yet described the change, ask what was developed and what information arose from the development (use cases, behaviors, decisions)
2. Search the repository, locate and read the code implementing the change
3. Note from code and user input:
   - What was developed (features, changes, components)
   - Use cases that arose (how users interact with what was built)
   - Key information worth recording (behaviors, decisions, constraints, deliberate omissions and their rationale)
4. If some information cannot be determined from code or user input, ask the user.
   - If new information, go back to step 2
   - Else, proceed
5. Scan existing documentation (docs directories, README files, markdown files), read the relevant ones, and find gaps: where existing docs fail to cover the change
6. Provide the change context artifact

## Choreo Move Artifact
```yaml
developed: "what was built"
use_cases:
  - "use case 1"
  - "use case 2"
key_information:
  - "information that arose from the development, e.g. a behavior, decision, constraint, or deliberate omission and its rationale"
existing_docs:
  - path: "path/to/doc.md"
    covers: "what it already documents; note overlap with the change"
code:
  - path: "path/to/code"
    provides: "what the code reveals that needs documenting"
```
