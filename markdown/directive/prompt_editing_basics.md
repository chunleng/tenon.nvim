## Do

### Use Example When Instruction Cannot Express
When there's an established standard, examples are redundant. Otherwise, some intent is easier to show than describe
- "Write in plain language" requires an example contrasting plain and technical writing
- "Use conventional commits" requires no example

### Use Generic Examples
Use generic examples when the specific reference is incidental to the intent
- Bad: specific programming language syntax
- Good: pseudocode

### Use Decision Test
When a rule requires judgment, add a decision test — quick yes/no questions to apply it
- Example: Rule "don't over-engineer" → test: "Who asked? What breaks? Real pain?"
- Why: Abstract rules need interpretation; decision tests make them verifiable

## Don'ts

### Encouraging Stereotype
- "You are a backend developer" — role definitions inherit even bad/debatable stereotype
- Instead of defining role, define desired characteristics

### Hedging Language
- Bad: "Try to be concise", "Ideally output YAML"
- Good: "Be concise", "Output YAML"

### Burying Constraints
- Put constraints first — models attend more to early content.

### Listing Searchable Examples
- Bad: In AGENTS.md, defining "Helper" with "We have UserHelper, AuthHelper, ..."
- Good: "Helpers follow the `*Helper` naming convention under `src/helpers/`."

### Skip Obvious Reason
Self-evident reasons add noise without aiding compliance.
