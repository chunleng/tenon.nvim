## Process
1. On re-entry from Review, address the identified gaps rather than rewriting from scratch
2. Draft using `decision_frame` and optional `exploration` from memory, Apply the Create or Update subsection according to `mode`
3. Write the document to `decision_frame.working_file_path`
4. Ask the user to confirm the draft or describe what to change. Incorporate requested changes and repeat confirmation until the user confirms

### Create
1. Structure a new document for decision-readiness using `decision`, `problem`, and `scope` from `decision_frame`
2. Include `alternatives` and their `trade_offs` from `exploration` when present. If `exploration` is absent, do not require new alternatives
3. Include a recommendation with rationale if applicable, and make genuine open questions explicit

### Update
1. Read the document at `working_file_path` and apply `requested_changes` from `decision_frame` in place. Preserve existing decisions and unaffected content, including existing alternatives
2. Check only content affected by `requested_changes`, using the working document for decision and problem context:
   - Ensure it is clear what's being decided and why it matters
   - Include a recommendation with rationale if applicable
   - Make genuine open questions explicit
3. Do not require new alternatives
