## Workflow

Each step yields artifact for next step. Next step reads prior artifact — never assumes unstated context. Artifacts internal → no chat output unless user asks.

### 1. Gather Context

- **Create**
  - Ask purpose, input types, output shape
  - User omits any → ask explicitly before proceeding
- **Change**
  - Read current prompt → identify sections touched by requested change
  - User requirements contradict → surface conflict, ask for resolution
  - Never remove constraints or behaviors not explicitly targeted by change; if change makes constraint redundant → remove it

**Yields**: user requirements

### 2. Draft & Apply

- **Guard**: add constraints for misleading instructions + likely failure modes
  - For each instruction, ask: "What input would cause model to violate this?"
  - Add constraint that prevents failure
  - Example: instruction says "translate to Spanish" → model often adds "Here's the translation:" → add "Output only the translation, no preamble"
- **Include Examples**
  - Never add examples that repeat what's already covered — every example must clarify beyond instructions
  - Default zero-shot: few-shot only when output shape is easier shown than described
- **Ordering: per instruction**
  - Guard precedes instruction it protects; examples last
  - Rationale: models attend more to early content; guard read before instruction
  - Bad: "Output JSON. Example: {...}. Never wrap in code blocks." — guard buried at end
  - Good: "Never wrap in code blocks. Output JSON. Example: {...}." — guard first

- **For workflow prompts**: each step must declare its yield
  - Example: step "Extract entities from text" → `**Yields**: list of (entity, type) tuples`

**Yields**: draft prompt text

### 3. Aggregate & Filter

- Deduplicate — after drafting, scan for any two rules that could fire on same input → merge or subordinate one
- Drop redundant restatements
- Necessity filter — for each instruction, name the specific failure it prevents. Cannot name one → remove it

**Yields**: deduplicated prompt text

### 4. Compact

- Compact to save tokens

**Yields**: compacted prompt text

### 5. Quality Check

**Core checks**

- No contradictions
- No ambiguity — no sentence reads two ways
- Examples justified — every example must clarify beyond instructions; ambiguous cases must be covered by example

**Anti-patterns**

- No hedging ("try to", "ideally") → imperatives: "Do X", "Never Y"
- No undefined references ("use the format above") → inline or name explicitly
- No intent preamble ("You are a...", "Your task is to...") → start with first substantive instruction
- Favor pattern directives ("Invoke [pattern]" / "Avoid [pattern]") over conditional triggers ("Do X when Y") — if conditional trigger is used and inexhaustible, state the default for unlisted conditions
- No filtering without enumeration — any instruction that asks agent to filter must include prior enumeration step.

Fail any → fix and restart from "Draft & Apply"

**Yields**: failures + locations (if any)

### 6. Verify

Mentally walk through representative inputs:
- Typical case — expected input/output
- Edge case — boundary conditions
- Adversarial case — input designed to expose weaknesses (e.g., user injects conflicting instruction mid-prompt)

**Yields**: pass/fail verdict

Fail → fix and restart from "Draft & Apply"

All checks pass → output prompt
