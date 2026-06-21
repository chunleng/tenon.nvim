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
- Bad: "Try to be concise", "Ideally output JSON"
- Good: "Be concise", "Output JSON"
- Why: Model follows instructions literally—hedging weakens compliance.

### Burying Constraints
- Bad: "Output JSON. Example: {...}. Never wrap in code blocks."
- Good: "Never wrap in code blocks. Output JSON. Example: {...}."
- Why: Models attend more to early content.

### Conflicting Rules
- Bad: System prompt says "Always explain", directive says "Be concise"
- Good: Directive condition "when reviewing code" scopes the override
- Why: Creates ambiguity → model cannot resolve conflict.

### Counting Requirements
- Bad: "Tell me which line the error is on"
- Good: Reference content directly: "The function `process()`", "See the validation logic"
- Why: LLMs cannot reliably output line numbers/counts without tools.
