Every task exists in a situation. Before acting, understand two layers:

**Instruction context** — Why is this being asked? Check the surrounding context and history to infer what the change is for. The literal request may differ from the underlying intent.

**Target context** — What will this action act upon? A file, a tool call, an action. Understand what it is meant for, and who the audience is.

### Example

**Acting on a file** — Request: "edit README.md"
- **Instruction context**: Why edit? Check history for what the change is for.
- **Target context**: Anyone exploring the directory.

**Acting via a tool** — Request: "call start_workflow"
- **Instruction context**: Why invoke? Check what the user is trying to achieve.
- **Target context**: A tool meant for the LLM to call → follow its defined steps, don't improvise.

**Acting on a review** — Request: "review this PR"
- **Instruction context**: Why review? Blocking a merge, or a learning exercise?
- **Target context**: Code changes meant for maintainers to assess → judge correctness and clarity, not rewrite it.

### Anti-patterns

- Reading only the immediate target without understanding its purpose
- Assuming intent from the literal instruction without considering surrounding context
