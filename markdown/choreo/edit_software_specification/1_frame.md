## Process
1. Identify the destination and mode from the user's request:
   - **Destination**: If the request names a specific platform to use, including through a platform document URL, use platform delivery. Otherwise, use file delivery
   - **Mode**: If a link or file identifies the current specification to edit, use update. Otherwise, use create
   - For platform creation, identify the creation location. For platform updates, fetch the current specification from its URL. For file updates, read the current specification. Use any pasted content as input
   - Ask only when the request is ambiguous or required destination details are missing
2. Interview the user for shared understanding of the document's decision:
   - Ask one question at a time, waiting for feedback before continuing
   - For each question, provide your recommended answer
   - If a fact can be found by searching the codebase or existing docs, look it up rather than asking the user
   - The decisions are the user's; put each one to them and wait for their answer
   - Establish whether this is creation or an update.
     - For updates, confirm the requested changes; preserve unaffected content and existing decisions. If the request changes an existing decision, use creation from scratch instead of updating it
     - For creation, resolve:
       - What is being decided or aligned on?
       - What problem or opportunity triggers this decision now?
       - What's in scope vs out of scope?
3. Prepare a working file without asking the user:
   - For platform delivery: a temporary file for drafting; initialize it with the fetched content for updates
   - For file delivery: use the destination path as the working file. Edit the existing file in place for updates or create a new file for creation, using pasted content as input when provided

## Choreo Move Artifact
### Create
```yaml
mode: create
decision:
  - What is being decided or aligned on
  - ...
problem: Why this decision matters now (trigger, current state, pain point)
scope: What's in scope vs out of scope for the new document
destination_type: platform | file
destination: Platform creation location or final file path
working_file_path: Temporary draft path for platform delivery; destination path for file delivery
```

### Update
```yaml
mode: update
requested_changes: Specific edits requested for the existing document
destination_type: platform | file
destination: Existing platform URL or final file path
working_file_path: Temporary draft path for platform delivery; destination path for file delivery
```
