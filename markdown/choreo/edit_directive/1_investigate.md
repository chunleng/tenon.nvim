## Process
1. Determine whether the user stated a problem or a proposed solution
   - Problem: user describes what went wrong (e.g. "the agent edits files without considering impact on other code")
   - Solution: user describes what they want the agent to do (e.g. "I want the agent to read files before editing")
2. If the user stated a solution, trace back to the root problem by asking "why" — repeat until you reach a concrete agent behavior problem
   - Example: "read files before editing" → "why?" → "edits broke other code" → "why?" → "didn't understand dependencies" → root problem identified
   - Do not proceed until the root problem is clear
3. Ask structured breadth questions to map the problem space. Ask all at once, then wait for the user's response:
   - What was the agent doing when the problem occurred?
   - What went wrong — what was the actual behavior?
   - What should have happened instead?
   - Is this a recurring pattern or a one-time occurrence?
4. Based on the answers, drill into specifics with targeted follow-up questions. Ask one question at a time, waiting for feedback before continuing. Continue until you and the user share a precise understanding of the root cause
   - Walk down each branch of the problem, resolving dependencies one-by-one
   - For each question, provide your recommended answer
   - If a fact can be found by reading existing directives in `markdown/directive/`, look it up rather than asking the user
   - The decisions are the user's — put each one to them and wait for their answer
   - Example: "what kind of impact?" → "function behaves differently after change?" → "contract change or internal-only?"
5. Summarize the root cause as a concise statement and confirm with the user. If the user disagrees or refines, loop back to process step 3 or 4 as needed

## Choreo Move Artifact
```yaml
root_cause: |
  <concise statement of the root cause>
```
