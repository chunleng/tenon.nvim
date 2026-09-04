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
   - **Update**: an existing directive should address the problem but doesn't — it needs to be extended, reworded, or have a missing scenario added
4. If the action is "no directive needed":
   - Tell the user why and ask for confirmation
   - If the user confirms, stop — the choreo ends here
   - If the user disagrees, re-evaluate and loop back to process step 2
5. Determine the directive type based on the root cause:
   - **Behavior-steering**: the agent needs rules about what to do or not do
   - **Knowledge-boosting**: the agent lacks information it needs to act correctly
   - This emerges from the analysis — do not ask the user to categorize
6. Present the action, type, and target file to the user with your reasoning
   - If the user has a stated preference (e.g. "I want a new directive") that differs from your analysis, explain why your analysis suggests a different action and let the user decide
7. Ask the user to confirm the analysis is correct:
   - If the action is "update": "Confirm this is correct? Make sure this directive was actually active when the problem occurred — the analysis is pointing at this directive being unclear or incomplete as the cause, so if it wasn't active, the update wouldn't have helped." If the user says it wasn't active, loop back to process step 2 to re-analyze and present a new proposal
   - If the action is "new": ask for confirmation directly
8. If the user confirms, proceed to the next move. If the user disagrees or refines, loop back to process step 2 or 3 as needed

## Choreo Move Artifact
```yaml
action: new | update
type: behavior-steering | knowledge-boosting
target_file: markdown/directive/{filename}.md
reasoning: |
  <why this action and type, referencing the root cause and existing directives>
```
