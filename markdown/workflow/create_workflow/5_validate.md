## Purpose
Validate steps flow properly between each other and instructions flow properly within each step

## Process

### Between Steps Validation
1. Check each step's output matches next step's input
2. Check goto_instructions conditions clear and non-overlapping
3. Check no dead ends (all paths lead somewhere)
4. Check no infinite loops without break condition

### Within Step Validation
1. Purpose → Process → Output alignment
2. Process steps sequential and actionable
3. Examples clarify rather than confuse
4. No buried critical constraints (place in Purpose section or first Process paragraph)

## Validation Checklist
- Step outputs match next step inputs
- goto conditions mutually exclusive
- Each step has clear Purpose, Process, Output
- Critical constraints appear in Purpose section or first Process paragraph
- No contradictory instructions within same step

## Workflow Step Output
If issues found:
```yaml
issues:
  - type: "flow"
    location: "step 2 → step 3"
    description: "..."
  - type: "internal"
    location: "step 4"
    description: "..."
```

If no issues:
```yaml
issues: []
```
