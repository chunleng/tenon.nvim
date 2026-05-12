## Purpose
Present workflow to user for final review and approval

## Process
1. Present complete workflow:
   - Show workflow.rs entry
   - Show each step's instruction file content
2. Ask user: "Does this workflow look correct?"
3. If user requests changes:
   - Do not edit on the spot
   - Pass to step 2 with change requests for redrafting
4. If user approves:
   - Confirm workflow complete
   - Output summary to chat

## Presentation Format
```
## Workflow: {title}
Trigger: {default_condition}

### Step 1: {step_title}
{instruction content}

### Step 2: ...
```

## Output
- If changes needed: `{"change_requests": ["..."]}`
- If approved: `{"status": "complete", "workflow_id": "..."}`
