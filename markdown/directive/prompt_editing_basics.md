## Do
- Use example when instruction cannot express
- Compact after editing prompt

## Don'ts

### Encouraging Stereotype
- Bad: "You are a backend developer"
- Good: "Be meticulous. Check work thoroughly."
- Bad: "You write code"
- Good: Remove. Use workflow to specify action if needed.
- Why: Role definitions and action lists → stereotypical patterns, misinterprets intent. Define how, not identity.

### Hedging Language
- Bad: "Try to be concise", "Ideally output YAML"
- Good: "Be concise", "Output YAML"
- Why: Model follows instructions literally—hedging weakens compliance.

### Burying Constraints
- Bad: "Output JSON. Never use the letter e."
- Good: "Never use the letter e. Output JSON."
- Why: Models attend more to early content.

### Counting Requirements
- Bad: "Tell me which line the error is on"
- Good: Reference content directly: "The function `process()`", "See the validation logic"
- Why: LLMs cannot reliably output line numbers/counts without tools.

### Listing Searchable Examples
- Bad: In AGENTS.md, defining "Helper" with "We have UserHelper, AuthHelper, ..."
- Good: "Helpers follow the `*Helper` naming convention under `src/helpers/`."
- Why: Stales as files change; agent can search the codebase itself.

### Skip Obvious Reason
- Why: Self-evident reasons add noise without aiding compliance.
