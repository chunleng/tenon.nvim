## Process
1. Repeat the following until there are no further questions to ask the user:
   - Establish what is currently known about the request
   - If useful, search the codebase for relevant information:
     - Existing features related to the request
     - Data flow and entry points in the relevant area
     - Embedded assumptions and implicit constraints in current implementation
     - Behaviors that might conflict with the request
   - Interview the user relentlessly until you reach a shared understanding:
     - Ask one question at a time, waiting for feedback before continuing
     - Walk down each branch of the decision tree, resolving dependencies one-by-one
     - For each question, provide your recommended answer
     - If a fact can be found by searching the codebase, look it up rather than asking the user
     - The decisions are the user's; put each one to them and wait for their answer
2. Ask the user directly in chat (do not use the `ask_question` tool): "Anything else I should be aware of?" Wait for the answer:
   - If the user adds new scope, go back to step 1 and re-run the interview loop to make sure everything is covered, then ask again
   - If the user confirms there is nothing more, provide the artifact

### What to ask
- Functional requirements: what the system must do
- Non-functional requirements: performance, security, privacy, scalability
- System responsibility boundaries: which system owns what responsibility. Focus on capabilities (what each system must be able to do), not internal structure (how the code is organized)

### What NOT to ask
- Which framework, technology, or library to use
- Implementation-level architecture (module separation, code structure)

## Choreo Move Artifact
Output the complete scope. Never output partial updates; always include the full scope even if only one item changed.

```yaml
scope:
  includes:
    functional:
      - "what the system must do"
    non_functional:
      - "performance, security, privacy, scalability constraints"
  excludes:
    - "what is explicitly out of scope"
```
