## Process
1. Read the written reference document
2. Statically verify against the codebase:
   - Every signature, parameter, default, and allowed value in the document matches the code
   - Completeness: no public item from the subject context artifact is missing from the document
3. If issues are found, navigate to move 2
4. If no issues are found, report the verification result to the user and ask them to confirm the reference is complete
5. If the user points out problems instead of confirming, treat their response as verification findings and navigate to move 2
6. If the user's response is unclear, ask again until they either confirm or point out problems

## Choreo Move Artifact
For navigating to move 2:
```yaml
- issue: "what does not match reality or is missing"
  fix: "what needs to be corrected"
```
