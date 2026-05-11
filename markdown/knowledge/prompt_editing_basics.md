## Do

- Use minimal words for necessary prompt change (Caveman mode compact)
- Use example when instruction cannot express

## Don'ts

### Role Definition in System Prompt

- Bad: "You are a backend developer"
- Good: "Be meticulous and careful. Check work thoroughly."
- Why: Role definitions lead LLM into stereotypical patterns. Define how agent should work, not what identity it has.

### Listing What Agent Does

- Bad: "You write code"
- Good: Remove such prompts. Use knowledge or workflow to document how to perform actions if needed.
- Why: Agent already knows to act when prompted. Stating what it does creates stereotypical behavior, misinterprets user intent.

### Hedging Language

- Bad: "Try to be concise", "Ideally output JSON"
- Good: "Be concise", "Output JSON"
- Why: Model follows instructions literally—hedging weakens compliance.

### Burying Constraints

- Bad: "Output JSON. Example: {...}. Never wrap in code blocks."
- Good: "Never wrap in code blocks. Output JSON. Example: {...}."
- Why: Models attend more to early content.

### Conflicting Rules

- Bad: System prompt says "Always explain", behavior says "Be concise"
- Good: Behavior condition "when reviewing code" scopes the override
- Why: Creates ambiguity, model cannot resolve conflict.
