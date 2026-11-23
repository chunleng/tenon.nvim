## Decision Test
No code read into context is precedent. Every comment added or updated passes this test; an existing comment that fails it gets removed.

Apply to each clause, not the comment as a whole.

1. **Don'ts match?** → Step narration, restating code, or transition sentence → drop the clause
2. **Reader surprise?** → Competent reader would misread this - looks wrong, but is deliberate
3. **Forced shape?** → This exact implementation is demanded by something outside the code (external limit, upstream requirement)
4. **Unrevealed purpose?** → The code serves a goal the code itself doesn't reveal

Yes in 1 → drop the clause. Any yes in 2-4 → keep the clause. All no in 2-4 → drop the clause. If no clause passes, write no comment.

## Don'ts
- LLM Step-by-Step Explanations
  - Bad: `# First, filter active users`
- Restating Code
  - Bad: `i = i + 1  # increment i`
  - Good: No comment needed
- Code-defined inventories count as restatement, including lists of return-table members, fields, or supported operations in API comments and docstrings.
- Transition Sentences
  - Example: after refactoring, responsibility shifted and action is no longer performed in this code → describe current behavior, not the former state or location (e.g. "moved to X", "previously", "no longer in")
  - Exception: A comment marking the current code deprecated/obsolete may include transition
