## Process
1. For each use case and key information in the change context, determine the documentation type it belongs to:
   - Tutorial: a newcomer needs to be walked through it hands-on to learn
   - How-to guide: a user with a practical goal needs steps to accomplish it
   - Reference: a user needs to look up its factual details (API, options, parameters)
   - Explanation: a user needs to understand why it is the way it is (design, decisions, context).
2. For each need, decide create vs. update using the change context's existing docs:
   - An existing doc already covers the type partially → update it
   - No suitable doc exists → create a new one
   - Discard needs that existing docs already cover completely
3. Present the assessment to the user, grouped by type; for each item include:
   - Type. i.e. `tutorial`, `how-to`, `reference`, `explanation`
   - Path to document (new/existing doc)
   - What the documentation is about
   - The use cases and key information this documentation must record
4. Get confirmation from user. If the user adjusts, update the assessment - mapping any new items to types per step 1 - and re-present (loop back to step 3)
5. Once confirmed, push the tasks to the work queue:
   - Group: `documentation_needed`
   - One task per assessment item, with details: the type, path, what the documentation is about, and the use cases and key information it must record
