## Process
1. Read the classification from workflow memory and the goal to identify the target text. The target may be file-based (file paths and sections) or inline text provided directly by the user. If re-entered from the Verify step, address each issue from the Verify step's output before proceeding.
2. For the target text:
   - Shorten passages classified as shorten — condense each to its brief form while preserving the core meaning
   - Drop passages classified as redundant
   - Preserve all load-bearing passages — conditions, constraints, decision criteria, and meaning-carrying examples — as-is
3. Write all compacted text in **neutral reference-manual prose**. The compacted form should read like documentation, not like commands. Avoid terse fragments that an agent might imitate in its own output.

   Style-contaminating (terse fragments — do not write this way):
   ```
   Cut rationale. Keep conditions. Merge duplicates.
   ```

   Neutral (same density, less infectious):
   ```
   Remove explanatory rationale. Preserve all conditional clauses. Deduplicate equivalent statements.
   ```

4. If the target is file-based, apply edits to the files in-place using the edit tool. If the target is inline text, output the compacted text directly.

## Guidelines
- Each behavioral instruction must remain individually identifiable after compaction — do not combine two rules into one sentence where their conditions or scopes differ
- When shortening a passage, preserve its core meaning — do not drop conditions, constraints, or exceptions embedded within it
- In Tenon prompt text, "workflow step" and "process step" are distinct terms — do not conflate them when compacting. For example, do not shorten "workflow step" to "step" when "process step" also appears in the same text.

## Wording Conciseness
When shortening passages, apply these word-level condensations:
- Remove filler openers that delay the actual point ("it should be noted that", "it is important to")
- Replace circumlocution with direct equivalents ("in order to" → "to", "due to the fact that" → "because")
- Drop redundant qualifiers that add no meaning ("basically", "essentially", "actually")
- Replace nominalization with verbs ("make a decision" → "decide", "give consideration to" → "consider")
- Replace wordy connectives with concise ones or drop them ("as a matter of fact" → "in fact", "for this reason" → "so")
