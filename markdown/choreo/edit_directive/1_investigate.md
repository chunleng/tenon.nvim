## Process
1. Determine whether the user stated a problem or a proposed solution
   - Problem: user describes what went wrong (e.g. "the agent edits files without considering impact on other code")
   - Solution: user describes what they want the agent to do (e.g. "I want the agent to read files before editing")
2. If the user stated a solution
   - Ask yourself why the proposed solution is needed to infer what problem it addresses. If the solution leaves gaps, use the answers to step 3 to fill them
3. ::tenon-chat:: Make 3 concurrent `ask_question` tool call: what the agent was doing when the problem occurred, what went wrong, and what should have happened instead; wait for the user's response
4. Based on the answers, resolve dependencies one at a time within each branch of the problem
   - If a fact can be found by reading existing directives in `markdown/directive/`, look it up rather than asking the user
   - ::tenon-chat:: One targeted follow-up question and your recommended answer; wait for the user's reply. Leave each decision to the user
   - Repeat step 4 until you and the user share a precise understanding of the root cause
5. ::tenon-chat:: A concise statement of the root cause and a question asking whether it is accurate; wait for the user's reply. If the user disagrees or refines the statement, return to step 3 or 4 as needed

## Artifact
```yaml
root_cause: |
  <concise statement of the root cause>
```
