## Process
1. Read the target text identified in the goal
2. For each passage, classify it as shorten, redundant, or load-bearing using the classification criteria below
3. Respect Tenon prompt structures when classifying. Directive conditions, choreo move artifacts, and constraint clauses are load-bearing by nature — do not classify structural elements as redundant or shorten
4. When you cannot determine whether a passage is shorten, redundant, or load-bearing, ask the user before proceeding. Do not guess.

## Classification Criteria
**Shorten** — can be condensed to a briefer form without losing meaning:
- Rationale and explanatory "why" text, when the rule itself is clear
- Examples that illustrate an already-clear rule (the rule carries the meaning; the example is reinforcement)
- Elaboration and context-setting prose that frames a rule but is not the rule itself
- Paired bad/good examples where the good example + rule already convey intent — the bad half identifies the anti-pattern but can be condensed rather than dropped
- Elaborate worked examples that can be condensed to brief scenarios while preserving the illustrating power

**Redundant** — can be dropped without losing meaning:
- Redundant restatements of the same point
- Negative restatements immediately after a positive constraint (e.g. "Output exactly: X. Do not add Y." — the constraint already implies the restriction)

**Load-bearing** — must be preserved as-is:
- Conditions ("when X", "unless Y") — these determine when a rule applies
- Constraints and boundaries ("never", "must", "always") — these are behavioral guardrails
- Decision tests and criteria — these drive judgment in ambiguous cases
- Examples that carry the meaning themselves (when the rule is vague without the example, the example IS the specification)

## Choreo Move Artifact
```yaml
- target: "passage or section identifier (e.g. file path + heading, or text snippet)"
  classification: "shorten | redundant | load-bearing"
  note: "what to shorten, drop, or preserve"
```
