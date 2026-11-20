## Process
1. If re-entered from Verify How-to, fix the issues in the verification findings artifact instead of rewriting the document; otherwise, create or update the how-to guide at the `doc_path` from the subject context artifact
2. Edit the document, following rules in "Writing Rules" section and in the bullet below:
   a. Title the document "How to X", saying exactly what the guide shows, e.g. "How to deploy the service to production"
   b. Include all mandatory sections:
      - **Goal**: what will be achieved, stated as the problem it solves
      - **Steps**: list the actions in the order the reader performs them
   c. Consider adding good-to-have sections where they add value:
      - **Prerequisites**: list only conditions the task requires but the guide does not establish. Common categories are access and permissions (read access to a repository), environment and tooling (a configured workspace or required tool version), and external dependencies (approval from the resource owner). Omit this section if none apply.
      - **Troubleshooting**: common failure points and their fixes
      - **Next steps**: where to go after completing the task

## Writing Rules
- Use `reader_knowledge` from the subject context artifact to decide how much detail a step needs. If readers know how to download a file, tell them which file to download, not how to use the download button.
- Use conditional imperatives where paths fork, e.g. "If you want x, do y"
- User perspective, not machinery: the goal drives the guide, tools are incidental
- No teaching or discussion mid-task - link to tutorials or explanations instead
- No exhaustive option listings - link to reference docs instead
- Practical usability over completeness: start and end in a meaningful place, omit the unnecessary
- Base every command, file path, and API on the subject context artifact - do not invent details
- Where a screenshot would help the reader (e.g. after a step with a visible result), insert a placeholder describing the screenshot needed, e.g. `![screenshot: the editor showing the newly created file]`
