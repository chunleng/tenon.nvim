## Process
1. If the user stated nothing before the workflow was triggered, ask what they want to accomplish
2. If mode is update, investigate the existing workflow — read its config and all instruction files and understand what it currently does. Requirements describe what the user wants to change, not a complete re-derivation of the workflow's purpose. Existing instruction files are the baseline — preserved as-is unless explicitly changed
3. Interview the user relentlessly until you reach a shared understanding. Repeat this step until there are no further questions. See the Interview Guide section below
   - Ask one question at a time, waiting for feedback before continuing
   - Walk down each branch of the decision tree, resolving dependencies one-by-one
   - For each question, provide your recommended answer
   - If a fact can be found by searching the codebase, look it up rather than asking the user
   - The decisions are the user's — put each one to them and wait for their answer
   - If user input conflicts with existing workflow behavior, ask whether the existing behavior should change or be preserved
4. Summarize the understood requirements as an array, output it, and confirm with the user before proceeding. If user requests changes, loop back to process step 3 (interview) and re-confirm

## Requirements DON'Ts
- Requirements describe what the workflow should accomplish, not how to structure it into steps. Do not list steps in the requirements — step design will happen later

## Interview Guide
**What to explore**:
- Problem to solve
- Trigger
- Expected outcome — including unintended side effects to avoid (e.g. a compaction workflow using caveman-style text could cause the agent to speak like a caveman when reading too much of it)

**What NOT to ask**: Don't ask the user to make design decisions — apply criteria yourself, present the result, let the user validate. For example, don't ask "should this be one step or two?" — apply the isolation criteria and present the design

## Workflow Step Artifact
```yaml
requirements:
  - ...
  - ...
```
