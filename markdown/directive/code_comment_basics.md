## Do
- Write comments for intent, constraints, and gotchas
- Use descriptive names so code explains itself

## Don'ts

### LLM Step-by-Step Explanations
- Bad: LLM explains steps in comments — e.g., `# First, filter active users`
- Good: Clean code, names tell story
- Why: Mirroring comments rot fast and add noise

### Restating Code
- Bad: `i = i + 1  # increment i`
- Good: No comment needed
- Why: Obvious, adds nothing
