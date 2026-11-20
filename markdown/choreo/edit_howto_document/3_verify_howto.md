## Process
1. Read the written how-to document
2. Statically verify against the codebase:
   - Every command the guide references exists and works as described
   - Paths presented as existing repository files resolve in the repository. If a later step refers to a file created earlier in the guide, verify that its path points to that file. Screenshot placeholders do not require an existing image.
   - Every API referenced exists with the described signature and behavior
3. If Prerequisites is present, check that each entry is a blocking condition established outside the task. Flag the section for removal if none apply.
4. Check step ordering: a reader who meets the prerequisites must be able to complete each step without relying on a later step.
5. Check step calibration: no step re-teaches what the `reader_knowledge` in the subject context artifact says the reader already knows
6. Check quadrant purity: the document contains no teaching or discussion mid-task, and no exhaustive option listings
7. Check the title convention: the title says exactly what the guide shows, in "How to X" form
8. Check all mandatory sections are present (Goal, Steps)
9. If issues are found, navigate to move 2
10. If no issues are found, ::tenon-chat:: report the verification result to the user and ask them to confirm the guide is complete
11. If the user points out problems instead of confirming, treat their response as verification findings and navigate to move 2
12. If the user's response is unclear, ::tenon-chat:: ask again
    - Continue until the user either confirms or points out problems

## Artifact
For navigating to move 2:
```yaml
- issue: "what does not match reality or is missing"
  fix: "what needs to be corrected"
```
