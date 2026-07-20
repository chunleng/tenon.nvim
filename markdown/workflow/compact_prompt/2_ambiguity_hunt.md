Hunt for ambiguous text in targets and resolve before compaction

Compaction can corrupt meaning when source text carries a double meaning. Resolve all ambiguity first.

## Process
1. Read targets from goal (workflow memory)
2. For each target, read each sentence. Assess whether it could carry a double meaning that compaction would corrupt — where removing or condensing words shifts the reader's interpretation
3. For each ambiguous sentence found:
   - Ask the user which meaning is intended
   - Apply the user's resolution to the text
   - Confirm the edit with the user
4. Repeat from process step 2 until no ambiguous sentence remains
5. When no ambiguity remains, proceed to the next step

## Workflow Step Artifact
None
