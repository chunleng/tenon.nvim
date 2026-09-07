## Process
1. If re-entered from Verify How-to, fix the issues in the verification findings artifact instead of rewriting the document; otherwise, create or update the how-to guide at the `doc_path` from the subject context artifact
2. Edit the document, following rules in "Writing Rules" section and in the bullet below:
   a. Title the document "How to X", saying exactly what the guide shows, e.g. "How to deploy the service to production"
   b. Include all mandatory sections:
      - **Goal**: what will be achieved, stated as the problem it solves
      - **Steps**: the logical sequence of actions toward the goal
   c. Consider adding good-to-have sections where they add value:
      - **Prerequisites**: what the reader needs before starting
      - **Troubleshooting**: common failure points and their fixes
      - **Next steps**: where to go after completing the task

## Writing Rules
- Write for a competent user: assume the basics are known, no hand-holding
- Use conditional imperatives where paths fork, e.g. "If you want x, do y"
- User perspective, not machinery: the goal drives the guide, tools are incidental
- No teaching or discussion mid-task - link to tutorials or explanations instead
- No exhaustive option listings - link to reference docs instead
- Practical usability over completeness: start and end in a meaningful place, omit the unnecessary
- Base every command, file path, and API on the subject context artifact - do not invent details
- Where a screenshot would help the reader (e.g. after a step with a visible result), insert a placeholder describing the screenshot needed, e.g. `![screenshot: the editor showing the newly created file]`
