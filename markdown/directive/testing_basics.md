## Don't

### Dynamic Imports
In test-driven development, test target may not exist yet.

Don't use dynamic imports because "test target doesn't exist"

### Testing for Absence of a Removed Component
When removing a feature, don't write tests asserting the removed component is gone (e.g., `assert content does not contain "step: 2"`).

Instead:
- Remove or edit tests that use the component being removed
- If the removed component is replaced by a new one, focus tests on the new requirement
