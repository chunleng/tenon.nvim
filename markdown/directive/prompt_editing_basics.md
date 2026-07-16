## Do

### Use Example When Instruction Cannot Express
When there's an established standard, examples are redundant. Otherwise, some intent is easier to show than describe.
- Bad: "Write in plain language" — vague, no example given
- Good: "Write in plain language" → show an example of plain vs. technical writing
- Bad: "Use conventional commits" → show a commit message example
- Good: "Use conventional commits" — model already knows the format
- Why: Examples anchor abstract instructions to concrete behavior. But if the model has a strong prior, examples add noise.

### Use Generic Examples
Use generic examples when the specific reference is incidental to the intent.
- Bad: `assert!(!content.contains("step: 2"))` (Specific language)
- Good: `assert content does not contain "step: 2"` (Pseudocode)
- Bad: "Cite sources in APA 7th edition format"
- Good: "Cite sources in your field's standard format"
- Why: Specific examples couple the instruction to one tool/framework/standard. Generic examples keep the instruction reusable and avoid implying a specific implementation.

### Use Decision Test
When a rule requires judgment, add a decision test — quick yes/no questions to apply it.
- Example: Rule "don't over-engineer" → test: "Who asked? What breaks? Real pain?"
- Why: Abstract rules leave room for interpretation. A decision test turns judgment calls into concrete, verifiable checks.

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
