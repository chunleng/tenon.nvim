## Don't

### Dynamic Imports
In test-driven development, test target may not exist yet.

Don't use dynamic imports because "test target doesn't exist"

### Testing for Absence of a Removed Component
When removing a feature, don't write tests asserting the removed component is gone (e.g., `assert content does not contain "step: 2"`).

Instead:
- Remove or edit tests that use the component being removed
- If the removed component is replaced by a new one, focus tests on the new requirement

### Straightforward Tests
Tests that pass but prove nothing.

**Mirrors production logic (tautology)**
Test recomputes the expected value using the same approach as the code under test. Shared bug → both pass while code is broken.
```
// Bad: re-implements the same logic
test totalAge:
    users = [{age: 10}, {age: 20}]
    assert totalAge(users) == sum of user.age for each user in users

// Good: expected value is independent (hardcoded)
test totalAge:
    users = [{age: 10}, {age: 20}]
    assert totalAge(users) == 30
```

**Only asserts execution, not correctness**
Proves code ran, not that it's right.
```
// Bad
test totalAge:
    users = [{age: 10}, {age: 20}]
    assert totalAge(users) returns a value

// Good
test totalAge:
    users = [{age: 10}, {age: 20}]
    assert totalAge(users) == 30
```

**Tests trivial code that cannot be wrong**
Logic so simple it has no meaningful failure mode — pure noise.
```
// Bad: assignment can't be wrong
test setName:
    user = User()
    user.setName("Alice")
    assert user.name == "Alice"
```
