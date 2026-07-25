## Process
1. Read the written document
2. Check decision-readiness — can a reader actually reach or evaluate a decision from this?
   - Is the problem clear? Does the reader understand what's being decided and why?
   - Are alternatives honest? Not strawmanned or missing obvious options?
   - If there's a recommendation, is the rationale convincing?
   - Are open questions explicit, not buried in prose?
   - Is the document concise enough for a reader to get through?
3. Decide based on the review:
   - **If gaps remain**: Identify each one specifically. Go to workflow step 3 with the gaps
   - **If decision-ready**:
     1. Confirm with the user that the document is ready
        - If the user identifies a gap, treat it the same as gaps remaining — go to workflow step 3
        - If the user has other feedback, resolve it and repeat this confirmation
     2. Finalize based on source type:
        - **Platform source** (platform_link or platform_paste): If a tool exists to update back to the source, use it. Otherwise, output the final document to chat for the user to copy back. Remove the working file
        - **File or new source**: The document is already in place
     3. Workflow ends

## Workflow Step Artifact
### If gaps remain
```yaml
gaps:
  - "specific gap in the document"
```
