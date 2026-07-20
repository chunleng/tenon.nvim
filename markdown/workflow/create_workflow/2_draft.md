## Purpose
Draft workflow structure and each step's instruction content

## Process
1. Create workflow metadata (id, title, description)
2. For each step from previous step, create instruction content
3. Write to files:
   - `{storage_path}/{workflow_id}/1_{step_name}.md`
   - `{storage_path}/{workflow_id}/2_{step_name}.md`
   - etc
4. Update `{config_path}/workflow.rs` with new workflow definition

## Drafting Guidelines
- Each step requires a "Process" section.
- Include "Workflow Step Artifact" according to workflow memory's goal `steps.artifact`
- Clear, minimal language
- Include examples when instruction abstract or complex
- Define goto_instructions with conditions and artifacts

## Understanding Tenon Workflow

### Components

- **Workflow**
  - `id`: string
  - `title`: string
  - `steps`: array
  - `description`: string - what this workflow does (agent uses this to decide when to trigger)

- **Step**
  - `title`: string - step name (logs)
  - `instruction`: file path (preferred) | inline text
  - `goto_instructions`: array

- **GotoInstruction**
  - `to`: "Next" | "Step(n)" | "EndWorkflow"
  - `condition`: string | null (null = always matches)
  - `output_to_workflow_memory`: string | null

### Advice

- Order conditions before catch-all (null): evaluated in order; null always matches → later conditions blocked
- `output_to_workflow_memory`: workflow state (e.g., goals) → persists for all steps

## Workflow Step Artifact
None
