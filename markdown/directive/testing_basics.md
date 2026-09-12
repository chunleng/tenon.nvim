## Don't

### Dynamic Imports
In test-driven development, the test target may not exist yet, so don't use dynamic imports.

### Testing for Absence of a Removed Component
When removing a feature, don't write tests asserting the removed component is gone.

Instead:
- Remove or edit tests that use the component being removed
- If the removed component is replaced by a new one, focus tests on the new requirement

### Straightforward Tests
Tests that pass but prove nothing.

**Mirrors production logic (tautology)**
Test recomputes the expected value using the same approach as the code under test.
```
// Bad
test totalAge:
    users = [{age: 10}, {age: 20}]
    assert totalAge(users) == sum of user.age for each user in users

// Good: hardcoded expected value
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
Logic so simple it has no meaningful failure mode.
```
// Bad: assignment can't be wrong
test setName:
    user = User()
    user.setName("Alice")
    assert user.name == "Alice"
```
