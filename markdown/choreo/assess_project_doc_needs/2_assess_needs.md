## Process
1. For each use case and key information in the change context, determine the documentation type it belongs to:
   - Tutorial: a newcomer needs to be walked through it hands-on to learn
   - How-to guide: a user with a practical goal needs steps to accomplish it
   - Reference: a user needs to look up its factual details (API, options, parameters)
   - Explanation: a user needs to understand why it is the way it is (design, decisions, context).
2. For each need, decide create vs. update using the change context's existing docs, following the decision tree in "Create vs. Update Decision Tree"
3. Present the assessment to the user, with two groups:
   - Documents to create or update, grouped by type; for each item include:
     - Type. i.e. `tutorial`, `how-to`, `reference`, `explanation`
     - Path to document (new/existing doc)
     - What the documentation is about
     - The use cases and key information this documentation must record
   - Docs related to the change that were considered as update candidates but need no update, so the user can verify the filtering; for each item include:
     - Path to document
     - Reason (the change does not alter what it covers)
4. Get confirmation from user. If the user adjusts, update the assessment - mapping any new items to types per step 1 - and re-present (loop back to step 3)
5. Once confirmed, push the tasks to the work queue:
   - Group: `documentation_needed`
   - One task per assessment item, with details: the type, path, what the documentation is about, and the use cases and key information it must record

## Create vs. Update Decision Tree

- Does an existing doc cover the need's subject?
  - No → create a new doc
  - Yes → proceed to next question
- Does the doc already cover the need completely?
  - Yes → discard the need
  - No → proceed to next question
- Does the change align with the doc's objective? Without an update, the doc's content would be wrong, incomplete, or misleading. A doc merely mentioning an entity involved in the change (e.g. the same function) does not count
  - Yes → update the doc
  - No → create a new doc
