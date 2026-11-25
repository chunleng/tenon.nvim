## Process
1. Based on the decision from move 2, determine the target file:
   - New: choose a descriptive filename under `markdown/directive/`: use `*_attitude.md` for behavior-steering, `*_basics.md` for knowledge-boosting, or a descriptive name if neither fits
   - Update: use the existing directive file
2. Research online for how to best encourage the desired agent behavior, or what resources/knowledge are needed to solve the problem. This step is optional: if you have no good search idea, skip the search rather than forcing one
   - If you researched, ::tenon-chat:: The findings and a question about which, if any, to incorporate; wait for the user's decision
3. Edit the target file to address the root cause from move 1, incorporating only the research findings the user chose. If updating an existing directive, modify it in place rather than rewriting from scratch
4. ::tenon-chat:: A summary of the current draft and how it addresses the root cause
5. ::tenon-chat:: A question asking whether the user approves the current draft; wait for the user's reply
6. Handle the reply:
   - If the user rejects the draft without specifying a change, ::tenon-chat:: A question about what they want changed; wait for their feedback before revising
   - If the user requests or authorizes an edit, return to step 3
   - If the user asks a question, ::tenon-chat:: The answer; then return to step 5 unless you propose an edit
     - If you propose an edit, ::tenon-chat:: The proposed edit and a question asking whether to make it; wait for the user's reply
     - If the user authorizes the edit, return to step 3. If they decline, return to step 5
     - Otherwise, ::tenon-chat:: An answer to any follow-up question or a clarification of the proposal, as needed, followed by a question asking whether to make the edit; wait for the next reply and repeat this decision process until the user authorizes or declines the edit
   - If no earlier branch applies and the reply does not clearly approve the current draft in response to the approval question, return to step 5
   - For example, "Ok" in direct answer to the approval question approves the draft. "Ok" in answer to a proposed edit authorizes only that edit.

## Drafting Guidelines
- Write each paragraph as a single unbroken line (soft wrap), not a fixed column width such as 80 characters
