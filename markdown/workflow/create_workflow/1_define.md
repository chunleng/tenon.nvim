## Purpose
Define workflow goal and step structure through user interaction. Critical: do not proceed until user confirms step structure

## Process
1. If goal not stated → ask: "What is the goal of this workflow?"
2. Ask clarifying questions when goal ambiguous, incomplete, or conflicting
3. Search for `require("tenon")` to find config location, search for existing workflow markdowns to find storage location (ask if ambiguous)
4. Derive `workflow_id` (snake_case) and `trigger_condition` from purpose
5. Propose step structure: `[{"step_title": "...", "purpose": "...", "input": "...", "output": "..."}]`
6. Present flow: verify step N output → step N+1 input match
7. Iterate until user approves

## Clarifying Questions

### Good
- Narrow scope: "Should this handle existing workflow updates or only new creation?"
- Resolve ambiguity: "When you say 'validate', should it be automated or user-reviewed?"
- Clarify boundary: "Is this for agent-prompting only, or any workflow type?"
- Surface hidden requirements: "Should the workflow handle errors or just happy path?"

### Bad
- Already answered: "What's the goal?" (when user just stated it)
- Too broad: "What do you want?" → ask specific questions instead
- Implementation detail: "Which data structure should I use?" → LLM decides
- Opinion-seeking: "Do you think step 2 is necessary?" → propose, let user reject

## Output
```json
{
  "workflow_id": "...",
  "trigger_condition": "...",
  "purpose": "...",
  "config_path": "...",
  "storage_path": "...",
  "steps": [
    {"step_title": "...", "input": "...", "output": "..."}
  ]
}
```
