## Process
1. Read the written tutorial document
2. Statically verify against the codebase:
   - Every command the tutorial references exists and works as described
   - Every file path exists
   - Every API referenced exists with the described signature and behavior
3. Check the step ordering: a newcomer following the steps in order, starting from the prerequisites, must succeed
4. Check all mandatory sections are present (Goal, Prerequisites, Steps, Expected result)
5. If issues are found, navigate to move 2
6. If no issues are found, report the verification result to the user and ask them to confirm the tutorial is complete
7. If the user points out problems instead of confirming, treat their response as verification findings and navigate to move 2
8. If the user's response is unclear, ask again until they either confirm or point out problems

## Choreo Move Artifact
For navigating to move 2:
```yaml
verification_findings:
  - issue: "what does not match reality or is missing"
    fix: "what needs to be corrected"
```
