## Do

- Comments → "why" (intent, constraints, gotchas)
- Code → "what" (names do this)

## Don'ts

### LLM Step-by-Step Explanations
- Bad: LLM explains steps in comments
- Good: Clean code, names tell story
- Why: Mirroring comments → noise, rot faster

**Bad: LLM explaining its work**
```
# First, filter active users
filtered = users where active
# Then sort by name
sorted = filtered sorted by name
# Return top 10
return first 10 of sorted
```

**Good: Let code speak**
```
active = users where active
return first 10 of (active sorted by name)
```

### Restating Code
- Bad: `i = i + 1  # increment i`
- Good: No comment needed
- Why: Obvious, adds nothing
