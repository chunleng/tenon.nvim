## Process
1. If re-entered from Verify Tutorial, fix the issues in the verification findings artifact instead of rewriting the document; otherwise, create or update the tutorial document at the `doc_path` from the subject context artifact
2. Edit the document, following rules in "Writing Rules" section and in the bullet below:
   a. Include all mandatory sections:
      - **Goal**: what the learner will have achieved by the end
      - **Prerequisites**: environment, tools, and knowledge needed before starting
      - **Steps**: the hands-on walkthrough - concrete, ordered, each step producing a visible result
      - **Expected result**: what success looks like at the end, so the learner can verify they did it right
   b. Consider adding good-to-have sections where they add value:
      - **Troubleshooting**: common errors and their fixes
      - **Next steps**: pointers to related how-to guides or reference docs
      - **FAQ**: questions that arise during the walkthrough

## Writing Rules
- Write for a newcomer: no unexplained jargon, no assumed knowledge beyond the prerequisites
- Base every command, file path, and API on the subject context artifact - do not invent details
- Where a screenshot would help the learner (e.g. after a step with a visible result), insert a placeholder describing the screenshot needed, e.g. `![screenshot: the editor showing the newly created file]`
