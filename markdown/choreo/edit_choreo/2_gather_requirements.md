## Process
1. If the user stated nothing before the choreo was triggered, ask what they want to accomplish
2. If choreo_id is null, determine it during the interview (process step 5): build on what the user stated; if the user stated a name, use it as-is, do not normalize or validate it against any convention. If no name is stated, generate one from the requirements; it must follow the `<verb>_*` format (e.g. `review_code`, `edit_choreo`)
3. If mode is update, investigate the existing choreo: read its config and all instruction files and understand what it currently does. Requirements describe what the user wants to change, not a complete re-derivation of the choreo's purpose. Existing instruction files are the baseline, preserved as-is unless explicitly changed
4. If mode is `create`, research online for the most up-to-date standard way people accomplish what the choreo aims to achieve. If research yields nothing relevant, proceed with the interview alone
5. Interview the user relentlessly until you reach a shared understanding. Repeat this step until there are no further questions. See the Interview Guide section below
   - Ask one question at a time, waiting for feedback before continuing
   - Walk down each branch of the decision tree, resolving dependencies one-by-one
   - For each question, provide your recommended answer
   - If a fact can be found by searching the codebase, look it up rather than asking the user
   - The decisions are the user's; put each one to them and wait for their answer
   - If user input conflicts with existing choreo behavior, ask whether the existing behavior should change or be preserved
   - If research findings exist, present them to the user and let them decide what to adopt; adopted findings flow into the requirements
6. Summarize the understood requirements as an array, output it, and confirm with the user before proceeding. If requirements changed, loop back to process step 5 (interview) and re-confirm

## Requirements DON'Ts
- Requirements describe what the choreo should accomplish, not how to structure it into moves; move design will happen later

## Interview Guide
**What to explore**:
- Problem to solve
- Trigger
- Expected outcome, including side effects to avoid (e.g. a compaction choreo using caveman-style text could make the agent speak like a caveman)

**What NOT to ask**: Don't ask the user to make design decisions; apply criteria yourself, present the result, let the user validate (e.g. don't ask "should this be one move or two?"; apply the isolation criteria and present the design)

## Choreo Move Artifact
```yaml
requirements:
  - ...
  - ...
```
