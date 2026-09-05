## Process
1. If re-entered from Verify Reference, fix the issues in the verification findings artifact instead of rewriting the document; otherwise, create or update the reference document at the `doc_path` from the subject context artifact
2. Edit the document, following rules in "Writing Rules" section and in the bullet below:
   a. Apply the template matching the `kind` from the subject context artifact:
      - **API**: per item - signature, parameters, return value, errors, example
      - **CLI**: per command - usage, options, arguments, examples
      - **Configuration**: per option - name, type, default, description
      - **Data model**: per entity - fields, types, constraints, relations
      - **Other**: structure the document to mirror the code
   b. Cover every item in the subject context artifact

## Writing Rules
- Dry, factual, neutral: no opinions, no discussion, no teaching
- Include a short usage example per item
- Base every signature, parameter, default, and allowed value on the subject context artifact - do not invent details
