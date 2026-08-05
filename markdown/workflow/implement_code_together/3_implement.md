## Process
1. If goal is to create a test, say "Test only goal, skipping implementation" and navigate to workflow step 1
2. Implement only what's needed to make the test pass:
  - Follow project coding standards (check AGENTS.md or project instructions)
  - Match existing code style
  - No scope creep/extra features
3. Verify implementation:
  - Build project
  - Run test (should pass now)
  - Run tests in same module/feature area
4. Summarize changes
5. Ask user to confirm: "Please confirm the implementation"
  - Confirm → format code and navigate to workflow step 1
  - Otherwise → use "Decision: Confirm vs. Reject vs. No Answer" to interpret the response

## Decision: Confirm vs. Reject vs. No Answer
**Cohesion test 1:** Is user asking a question?
- question → "No answer" → reply to user and ask to confirm again
- else → continue

**Cohesion test 2:** Is user asking for something part of the goal?
- part of goal → "Rejected" → revise based on feedback and loop to process step 1
- else → "Confirm" → format code and navigate to workflow step 1

## Workflow Step Artifact
Nothing
