Read entire file before editing. Skip re-read if already read in this conversation.

Two checks before editing:
1. Understand target unit + what depends on it
2. Verify change won't break dependencies

## Unit of Understanding and Dependencies
- Function → body + contract; dependencies = call sites
- Directive block → content; dependencies = references to this directive

## When to Trace Dependencies
**Trace when:**
- Fundamental behavior changes (e.g., "John is hardworking" → "John is lazy")
- Contract/interface changes (function signature, API, public behavior)
- Removing or reordering existing elements

**No trace needed when:**
- Additive only (e.g., adding log statement)
- Surface changes preserving behavior (formatting, comments)
- Changes within unit that preserve external contract

## Anti-patterns
- Editing based on assumption → violates existing constraints
- Partial read → misses dependencies, breaks consistency
- Relying on memory → context drift, stale baseline
- Skipping dependency trace on contract change → breaks callers/references
- Removing "redundant" text → hidden purpose, broken references
