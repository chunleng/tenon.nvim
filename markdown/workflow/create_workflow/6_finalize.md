## Purpose
Present workflow → user for final review/approval

## Process
1. Present changes from current iteration:
   - Show workflow.rs entry if created/modified
   - Show instruction file content if created/modified
2. Ask user: "Does this workflow look correct?"
3. If user requests changes:
   - No on-the-spot edits
   - Pass to step 2 w/ change requests for redrafting
4. If approved, complete workflow

## Presentation Format
```
## Workflow: {title}
Description: {description}

### Step 1: {step_title}
{instruction content}

### Step 2: ...
```

## Output
- Changes needed: `change_requests: ["..."]`
- Approved: `status: "complete", workflow_id: "...", instruction: "compact created prompt"`
