## Process
1. Read the written how-to document
2. Statically verify against the codebase:
   - Every command the guide references exists and works as described
   - Every file path exists
   - Every API referenced exists with the described signature and behavior
3. Check the step ordering: a competent user following the steps in order, starting from the prerequisites if present, must succeed
4. Check step calibration: no step re-teaches what the `reader_knowledge` in the subject context artifact says the reader already knows
5. Check quadrant purity: the document contains no teaching or discussion mid-task, and no exhaustive option listings
6. Check the title convention: the title says exactly what the guide shows, in "How to X" form
7. Check all mandatory sections are present (Goal, Steps)
8. If issues are found, navigate to move 2
9. If no issues are found, report the verification result to the user and ask them to confirm the guide is complete
10. If the user points out problems instead of confirming, treat their response as verification findings and navigate to move 2
11. If the user's response is unclear, ask again until they either confirm or point out problems

## Choreo Move Artifact
For navigating to move 2:
```yaml
- issue: "what does not match reality or is missing"
  fix: "what needs to be corrected"
```
