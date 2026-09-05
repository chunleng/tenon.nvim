## Process
1. If the user has not yet provided the subject or the information to include, ask for it
2. Scan existing documentation for a document that already covers the subject
3. Explore the repository to ground the tutorial:
   - Locate the code relevant to the subject
   - Identify the commands, file paths, and APIs a newcomer would actually use
   - If an existing document covers the subject, read it to determine what to keep and what to update
4. If information needed for the tutorial cannot be determined from the user's input or the code, ask the user, then go back to step 3

## Choreo Move Artifact
```yaml
title: "title of the tutorial document"
changes:
  - "what was developed or changed, if updating an existing document"
use_case: "how users interact with it, step by step"
key_information:
  - "behaviors, decisions, constraints worth recording"
doc_path: "path of the tutorial document to create or update"
```
