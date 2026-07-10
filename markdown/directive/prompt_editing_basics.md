## Do
- Use example when instruction cannot express
- Compact after editing prompt

## Don'ts

### Encouraging Stereotype
- Bad: "You are a backend developer"
- Good: "Be meticulous and careful. Check work thoroughly."
- Bad: "You write code"
- Good: Remove. Use workflow to document if needed.
- Why: Role definitions and action lists → stereotypical patterns, misinterprets intent. Define how agent works, not identity.

### Hedging Language
- Bad: "Try to be concise", "Ideally output YAML"
- Good: "Be concise", "Output YAML"
- Why: Model follows instructions literally—hedging weakens compliance.

### Burying Constraints
- Bad: "Output YAML. Example: key: value. Never wrap in code blocks."
- Good: "Never wrap in code blocks. Output YAML. Example: key: value."
- Why: Models attend more to early content.

### Conflicting Rules
- Bad: System prompt says "Always explain", directive says "Be concise"
- Good: Directive condition "when reviewing code" scopes the override
- Why: Creates ambiguity → model cannot resolve conflict.

### Counting Requirements
- Bad: "Tell me which line the error is on"
- Good: Reference content directly: "The function `process()`", "See the validation logic"
- Why: LLMs cannot reliably output line numbers/counts without tools.

### Listing Searchable Examples
- Bad: In AGENTS.md, defining "Helper" with "We have UserHelper, AuthHelper, OrderHelper, ..."
- Good: "Helpers follow the `*Helper` naming convention under `src/helpers/`."
- Why: The list goes stale as files are added/removed. The agent can search the codebase itself.
