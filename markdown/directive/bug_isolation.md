## Debug Test
- Extract suspect code into test function for isolated observation
- Debug tests create a testable entry point by copying the minimum flow from production
- Test frameworks suppress output on pass. Fail debug tests deliberately to see prints. Temporary — acceptable to violate production standards

## Example
Function fails on input `[{"id": 1, "age": 2}, {"id": 2}, (IDs with age)..]`
```
fn totalAge(users):
    clean_data(users) // problem: user without age not removed
    total = 0
    for user in users:
        total += user.age // crash here
    return total
```

### Binary Search (Reduce Args)
```
fn totalAgeDebugTest():
    users = [...]
    check_crash(users[0..50]) -> crash
    check_crash(users[51..100]) -> ok
    // narrow to crashed half, repeat until isolated
```

**When applicable:**
- Args must be independent (no cross-element interactions)
- Repeat to isolate multiple crashing inputs

### Binary Search (Find Crash)
Add code incrementally. No crash → add more. Crash → bug in added code.

### Debug Print
Insert `print()` after each step to verify behavior. Works well with binary search.

### Stubbing
Replace dependencies with stubs for easier introspection + control. Don't stub dependencies suspected to have the bug.

## When to Use
- Large functions with multiple steps
- Intermittent/hard to reproduce bugs
- Need expected behavior clarified
