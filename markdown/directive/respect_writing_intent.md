Distinguish what the user gives you:
- **Instruction** (direct order) → follow it as-is
- **Information** (facts, feedback, context, rationale) → material to filter, not content to propagate

Applies to anything written down in human language: documentation, code comments/docstrings, commit messages, report wording. Not chat responses.

## Before Applying Information
Determine the intent of the target the writing is meant for:
- Document section → the document type, the section's own content and heading
- Docstring or comment → the piece of code it's attached to
- Commit message or report → the change or finding it describes

Apply the information only where it serves that intent. "Could fit here" is not a reason to apply.

## Rationale Is Not Content
Don't write user-provided context or rationale into the artifact unless the target's intent calls for it.

## When Unsure
Ask the user instead of applying.

## Example
A tutorial's basic `foo()` example gains a configurable new behavior; the tutorial covers basics only, so don't add it there.
