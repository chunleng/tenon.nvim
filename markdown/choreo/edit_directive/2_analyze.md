## Process
1. Read all existing directives in `markdown/directive/`
2. Compare the root cause against existing directives:
  - Does any existing directive already fully address this problem?
  - Does any existing directive partially address it?
  - Is the root cause a gap in an existing directive (e.g. the directive exists but doesn't cover this scenario)?
  - Is the root cause a different problem from what an existing directive covers, even if symptoms are similar?
3. Determine the action based on the comparison:
  - **No directive needed**: the problem is already fully covered, is a one-off, or is not a directive issue (e.g. a choreo issue, a tool configuration issue)
  - **New**: the root cause is a problem not addressed by any existing directive
  - **Update**: an existing directive should address the problem but doesn't
4. If the action is "no directive needed":
  - ::tenon-chat:: the reason to the user and ask for confirmation
  - If the user confirms, stop — the choreo ends here
  - If the user disagrees, re-evaluate and loop back to process step 2
5. Determine the directive type based on the root cause:
  - **Behavior-steering**: the agent needs rules about what to do or not do
  - **Knowledge-boosting**: the agent lacks information it needs to act correctly
  - This emerges from the analysis; do not ask the user to categorize
6. If the user obviously asked to change a directive and the analysis result agrees with what the user asked for, skip the confirmation and proceed directly to the next move
  - Otherwise, ::tenon-chat:: referring to the Confirmation Request Guidelines
7. Wait for the user's confirmation
  - If the action is "update" and the user says the directive wasn't active, loop back to process step 2 to re-analyze and present a new proposal
8. If the user confirms, proceed to the next move. If the user disagrees or refines, loop back to process step 2 or 3 as needed

### Confirmation Request Guidelines
- The action, type, and target file, with your reasoning
- Ask the user to confirm the analysis is correct
- If the action is "update", include: "Confirm this is correct? Make sure this directive was actually active when the problem occurred."
- If the user has a stated preference (e.g. "I want a new directive") that differs from your analysis, explain why your analysis suggests a different action and let the user decide

## Choreo Move Artifact
```yaml
action: new | update
type: behavior-steering | knowledge-boosting
target_file: markdown/directive/{filename}.md
reasoning: |
  <why this action and type, referencing the root cause and existing directives>
```
