## Process
1. Read the document at `decision_frame.working_file_path` from memory
2. Apply the Create or Update subsection according to `mode` in `decision_frame`. Within its review boundaries, check:
   - Is it clear what's being decided and why?
   - If there's a recommendation, is its rationale convincing?
   - Are genuine open questions explicit?
   - Is the document concise enough to read?
3. If gaps remain, identify them specifically for revision and navigate to move 3
4. If no gaps remain, ask the user to confirm the document is ready. Treat requested document changes as revisions; resolve other feedback and repeat confirmation
5. After user confirmation, finalize using `decision_frame` from memory (`mode`, `destination_type`, `destination`, `working_file_path`):
   - **Platform** (`destination_type: platform`): If an appropriate tool exists, create a new document at `destination` for creation or update the URL in `destination` for updates. Otherwise, present the final document in chat for the user to copy to the platform. Remove `working_file_path` only after successful delivery
   - **File** (`destination_type: file`): `working_file_path` is the final deliverable at `destination`

### Create
1. Compare the document against `decision`, `problem`, and `scope` in `decision_frame`
2. If `exploration` is present in memory, check that `alternatives` are presented honestly with their `trade_offs` and without missing obvious options. If `exploration` is absent, do not treat the absence of new alternatives as a gap

### Update
1. Review only content affected by `requested_changes` in `decision_frame`, using the written document for decision and problem context
2. Check that `requested_changes` were made without changing unaffected content or settled decisions. Do not require new alternatives
3. Distinguish defects introduced by the edit from unrelated pre-existing issues; do not expand beyond `requested_changes` without user approval

## Choreo Move Artifact
### If gaps remain
```yaml
gaps:
  - Specific gap in the document
```
