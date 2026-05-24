## Purpose
Ask user for next incremental goal → end workflow if user says "stop". Information-gathering only — no code implementation.

## Process
1. Ask user: "What's the next incremental goal you'd like to work on?"
2. Wait for response
3. If user says "stop", "done", "that's all", or indicates completion → end workflow
4. If user provides goal → investigate
  a. questions → clarify directly with user → loop to investigate
  b. all clear about task → proceed next step

## Output
```json
{
  "goal": "clear description of the incremental goal user wants to achieve",
  "files": ["relevant files if mentioned"],
  "context": "any additional context provided by user"
}
```

## Example
**User provides goal:**
```json
{
  "goal": "Add empty string validation to password field",
  "files": ["src/auth/password.rs"],
  "context": "Should return user-friendly error message"
}
```
