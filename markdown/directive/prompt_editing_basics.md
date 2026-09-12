## Do

### Use Example When Instruction Cannot Express
Examples are redundant when an established standard exists; otherwise some intent is easier to show than describe
- "Write in plain language" needs an example contrasting plain and technical writing
- "Use conventional commits" needs no example

### Use Generic Examples
When the specific reference is incidental to the intent, use generic examples
- Bad: specific programming language syntax
- Good: pseudocode

### Use Decision Test
When a rule requires judgment, add a decision test — quick yes/no questions to apply it
- Example: Rule "don't over-engineer" → test: "Who asked? What breaks? Real pain?"

## Don'ts

### Encouraging Stereotype
- Role definitions like "You are a backend developer" inherit even bad/debatable stereotypes
- Define desired characteristics instead of roles

### Hedging Language
- Bad: "Try to be concise", "Ideally output YAML"
- Good: "Be concise", "Output YAML"

### Burying Constraints
- Put constraints first — models attend more to early content.

### Listing Searchable Examples
- Bad: In AGENTS.md, listing "We have UserHelper, AuthHelper, ..."
- Good: "Helpers follow the `*Helper` naming convention under `src/helpers/`."

### Skip Obvious Reason
Self-evident reasons add noise without aiding compliance.
