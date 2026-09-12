## Process
1. If the user has not yet provided the subject, ask what the explanation document should cover
2. If the user's request contains more than one subject (more than one distinct why question), narrow it down to one before proceeding:
   a. Ask the user to pick one subject to focus on for this run
   b. For the remaining subjects, confirm with the user which actually need to become documents; drop the ones that do not
   c. Push the remaining confirmed-necessary subjects to the work queue so they get handled automatically in subsequent runs
3. Scan existing documentation for a document that already covers the subject, and note any closely related documents on the same subject
4. Explore the repository to ground the explanation:
   - Locate the code relevant to the subject
   - Identify the facts the explanation will rely on: what exists, how it behaves, what it interacts with
   - If an existing document covers the subject, read it to determine what to keep and what to update
5. If the rationale or reasoning behind a design decision cannot be determined from the user's input, the code, or existing docs, ask the user. Then go back to step 4

## Choreo Move Artifact
```yaml
subject: "what the explanation document covers"
doc_path: "path of the explanation document to create or update"
related_docs:
  - "paths of closely related documents on the same subject"
facts:
  - "code facts the explanation relies on, e.g. what exists, how it behaves"
decisions:
  - decision: "what was decided"
    rationale: "why it was decided this way, from the user, code, or docs"
```
