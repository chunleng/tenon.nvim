## Process
1. Read the written explanation document
2. Statically verify:
   - Every factual claim about the code matches the codebase
   - Every rationale is grounded in the subject context artifact - nothing invented
   - Internal consistency: the document does not contradict itself
   - No contradictions with the closely related documents from the subject context artifact
   - Quadrant purity: the document contains no instruction (steps) or reference-style exhaustive listings
   - Bounded subject: the document answers a clear why question, not an open-ended survey
3. If issues are found, navigate to move 2
4. If no issues are found, report the verification result to the user and ask them to confirm the explanation is complete
5. If the user points out problems instead of confirming, treat their response as verification findings and navigate to move 2
6. If the user's response is unclear, ask again until they either confirm or point out problems

## Choreo Move Artifact
For navigating to move 2:
```yaml
- issue: "the mismatch, ungrounded rationale, or contradiction"
  fix: "what needs to be corrected"
```
