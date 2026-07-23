## Workflow

1. Check all categories:
    * Correctness → Logic errors, race conditions, null/edge cases, crashes, environment coupling
    * Security → Injection, auth bypasses, data exposure, input validation
    * Clarity → Unclear naming, missing context, magic numbers
    * Maintainability → Tight coupling, fragile interfaces, god objects
    * Style → Internal inconsistencies only (not style guide violations)

2. **Filter findings**:
    * Drop compilation errors → Build catches them
    * Drop linter warnings → Already accepted (e.g., nesting depth, function length)
    * Drop unchanged code → Review diff only
    * Filter by context (first match wins):
        + Test code → Keep correctness only
        + Prototype/internal tool → Lenient, drop minor issues
        + Frontend client → Keep UX-blocking issues
        + Core library → Keep all, strict
        + Data layer → Keep correctness + security
        + Scripts/utilities → Keep correctness only

## Output

### Output line numbers

* Include for specific code references
* Ranges for multi-line issues → L45-52
* Single line for point issues → L78

### Examples

**No blockers** → Output exactly:

```
LGTM!
```

**Blockers found** → Output:

```
<One-line summary of main concern>

## 1. **Race condition in user fetch** (src/api/user.rs L45-52)

Concurrent requests can overwrite `self.cache` without synchronization. Causes stale data on rapid navigation.

Fix: Wrap in `Mutex` or use `DashMap` for concurrent access.

## 2. **SQL injection via sort param** (src/db/queries.go L78)

User input `sortBy` concatenated directly into query string.

Impact: Attacker can execute arbitrary SQL.

Fix: Whitelist allowed sort columns: `["created_at", "name", "updated_at"]`.
```
