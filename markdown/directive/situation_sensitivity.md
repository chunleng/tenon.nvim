Every task exists in a situation. Before acting, understand two layers:

**Instruction context** — Why is this being asked? Check the directive and chat history to infer what the instruction means
**Target context** — What the text that you output is meant for? Who's the target audience?

- If answer to above context is unclear, ask user for clarification
- With the context gather, determine:
  - Tone to use for the target audience when outputting
  - Necessary information in such context

## Example

**Context**: Chat history shows `./docs/feature/login.md` was added; AGENTS.md says `./docs/feature` holds feature specs; `login.md` says "Anyone can login."
**User Request**: Add who "anyone" is?
**Conclude**:
- Instruction context: Read `login.md` and replace "anyone" with the exact login group
- Target context: `login.md` is a feature spec for repository developers

**Context**: No chat history; `./test.md` is empty.
**User Request**: Add common software testing rule.
**Conclude**:
- Instruction context: Ambiguous — ask "Why are we setting up testing rule?" and "What kind of software? Web, mobile?"
- Target context: Unknown what `test.md` is for or who reads it — ask user

## Target Audience Misconception
- **Output determines audience**: "Review PR"
    - Wrong: PR (object mentioned) is meant for developer to read
    - Correct: Review comment (LLM output) is meant for PR author
