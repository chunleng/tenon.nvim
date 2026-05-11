Never edit without reading. Read entire file first.

Two checks before editing:
1. Understand target unit + what depends on it
2. Verify change won't break dependencies

## Unit of Understanding and Dependencies

- Function → body + contract; dependencies = call sites
- Knowledge block → content; dependencies = references to this knowledge

## When to Trace Dependencies

**Trace when:**
- Fundamental behavior changes (e.g., "John is hardworking" → "John is lazy")
- Contract/interface changes (function signature, API, public behavior)
- Removing or reordering existing elements

**No trace needed when:**
- Additive only (e.g., adding log statement)
- Surface changes preserving behavior (formatting, comments)
- Changes within unit that preserve external contract

## Example

### When to Trace Dependencies

**Change requires trace:**
```
// Before: sum() returns number
fn sum(a, b) { return a + b }

// After: sum() returns string
fn sum(a, b) { return String(a + b) }
```
→ Callers expecting number break. Must trace callers.

**Change needs no trace:**
```
// Before
fn sum(a, b) { return a + b }

// After
fn sum(a, b) { print(a, b); return a + b }
```
→ Behavior unchanged, logging added. No caller affected.

### Reading Unit Prevents Breakage

**Change request:** "field_name for `total += user[field_name]` should look up from user.data"

**Without reading unit:**
```
total += user.data[field_name]  // changed only this line
```
→ print() still uses `user[field_name]`, inconsistent lookup.

**With reading unit:**
```
fn sumByField(users, field_name):
    for user in users:
        print(user[field_name])
        total += user[field_name]
```
→ See both uses. Change both consistently:
```
print(user.data[field_name])
total += user.data[field_name]
```

## Anti-patterns

- Editing based on assumption → violates existing constraints
- Partial read → misses dependencies, breaks consistency
- Relying on memory → context drift, stale baseline
- Skipping dependency trace on contract change → breaks callers/references
- Removing "redundant" text → hidden purpose, broken references
