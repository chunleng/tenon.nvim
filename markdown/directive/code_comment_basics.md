## Do

- Comments → "why" (intent, constraints, gotchas)
- Code → "what" (names do this)

## Don'ts

### LLM Step-by-Step Explanations

- Bad: LLM explains steps in comments
- Good: Clean code, names tell story
- Why: Mirroring comments → noise, rot faster

Example:

**Bad: LLM explaining its work**
```python
# First, we filter the users
filtered = [u for u in users if u.active]
# Then we sort them
sorted_users = sorted(filtered, key=lambda u: u.name)
# Finally, return the top 10
return sorted_users[:10]
```

**Good: Let code speak**
```python
active_users = [u for u in users if u.active]
return sorted(active_users, key=lambda u: u.name)[:10]
```

### Restating Code

- Bad: `i += 1  # increment i`
- Good: No comment needed
- Why: Obvious, adds nothing
