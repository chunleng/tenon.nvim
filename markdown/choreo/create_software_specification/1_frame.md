## Process
1. Interview the user to reach a shared understanding of the decision the document must support:
   - Ask one question at a time, waiting for feedback before continuing
   - For each question, provide your recommended answer
   - If a fact can be found by searching the codebase or existing docs, look it up rather than asking the user
   - The decisions are the user's — put each one to them and wait for their answer
   - Questions to resolve:
     - What is being decided or aligned on?
     - What problem or opportunity triggers this decision now?
     - What's in scope vs out of scope?
2. Determine the source type and ingest content:
   - **Platform link**: User provided a URL (e.g. GitHub issue). Fetch the content using the appropriate tool
   - **Platform paste**: User pasted document content directly. Use that content as the starting point
   - **File**: User specified a file path that already exists. Read the existing content
   - **New** (default): No source provided. Start from scratch
3. Create a working file without asking the user:
   - For platform sources: a temporary file for drafting
   - For file/new sources: this file is the final deliverable

## Choreo Move Artifact
```yaml
decision: What is being decided or aligned on
problem: Why this decision matters now (trigger, current state, pain point)
scope: What's in scope vs out of scope
source_type: platform_link | platform_paste | file | new
working_file_path: Where the document will be written and edited
```
