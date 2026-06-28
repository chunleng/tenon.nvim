Every task exists in a situation. Before acting, understand two layers:

**Instruction context** — Why is this being asked? Check the directive and chat history to infer what the instruction means
**Target context** — What the text that you output is meant for? Who's the target audience?

- If answer to above context is unclear, ask user for clarification
- With the context gather, determine:
  - Tone to use for the target audience when outputting
  - Necessary information in such context

## Example

**Context**:
- Chat history contains: Added "./docs/feature/login.md"
- AGENTS.md mention: "./docs/feature" contains feature specification of the system
- `login.md` contains: Anyone can login
**User Request**: Add who "anyone" is?
**Conclude**:
- Instruction context: User meant "use tool to read `login.md` content and change anyone to exact group using the login"
- Target context: `login.md` is feature specification meant for developer of the repository to read

**Context**:
- No chat history
- `./test.md` is empty
**User Request**: Add common software testing rule
**Conclude**:
- Instruction context: User is ambiguous, ask series of relating questions to understand context:
  - "Why are we setting up testing rule?"
  - "What kind of software are we testing? Web, mobile?"
- Target context: Unable to derive what `test.md` is and who it's for, ask user

## Target Audience Misconception
- **Output determines audience**: "Review PR"
    - Wrong: PR (object mentioned) is meant for developer to read
    - Correct: Review comment (LLM output) is meant for PR author
