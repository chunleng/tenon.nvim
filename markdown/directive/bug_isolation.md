## Debug Test
- Extract suspect code into test function for isolated observation
- Running arbitrary code often hard. Debug tests create testable entry point by copying minimum flow from production
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
// 1st
fn totalAgeDebugTest():
    users = [...]
    check_crash(users[0..50]) -> crash
    check_crash(users[51..100])

// 2nd, check crashed group
fn totalAgeDebugTest():
    users = [...]
    check_crash(users[0..25]) -> crash
    check_crash(users[26..50])
// Repeat until isolated
```

**When applicable:**
- Args must be independent (no cross-element interactions)
- Repeat to isolate multiple crashing inputs

### Binary Search (Find Crash)
Add code incrementally. No crash → add more. Crash → bug in added code.
```
// Round 1
fn totalAgeDebugTest():
    users = [...]
    clean_data(users)
    total = 0
    // No crash → continue

// Round 2
fn totalAgeDebugTest():
    users = [...]
    clean_data(users)
    total = 0
    for user in users:
        pass  // No crash → add body

// Round 3
fn totalAgeDebugTest():
    users = [...]
    clean_data(users)
    total = 0
    for user in users:
        total += user.age  // Crash → bug in body
```

### Debug Print
```
fn totalAgeDebugTest():
    users = [...]
    clean_data(users)
    print(users)  // verify step behaves as expected
    total = 0
    for user in users:
        print(user)
        total += user.age
    print(total)
```
Works well with binary search

### Stubbing
Replace dependencies with stubs for easier introspection + control.
```
fn emailTotalAge():
    users = fetch_users()
    clean_data(users)
    total = 0
    for user in users:
        total += user.age
    email(total)

fn emailTotalAgeDebugTest():
    users = [...] // stub
    clean_data(users)
    total = 0
    for user in users:
        total += user.age
    print(total)
```
Don't stub dependencies suspected to have the bug

## When to Use
- Large functions with multiple steps
- Intermittent/hard to reproduce bugs
- Need expected behavior clarified
