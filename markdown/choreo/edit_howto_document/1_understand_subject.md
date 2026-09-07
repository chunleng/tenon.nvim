## Process
1. If the user has not yet provided the task or the information to include, ask for it
2. Scan existing documentation for a document that already covers the task
3. Explore the repository to ground the how-to guide:
   - Locate the code relevant to the task
   - Identify the commands, file paths, and APIs a user would actually use
   - If an existing document covers the task, read it to determine what to keep and what to update
4. If information needed for the guide cannot be determined from the user's input or the code, ask the user, then go back to step 3

## Choreo Move Artifact
```yaml
task: "the specific, real-world problem the guide solves"
changes:
  - "what was developed or changed, if updating an existing document"
key_information:
  - "behaviors, decisions, constraints worth recording"
doc_path: "path of the how-to document to create or update"
```
