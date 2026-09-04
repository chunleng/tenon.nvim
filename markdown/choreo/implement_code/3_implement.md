## Process
1. Read the target files first — understand what exists before changing it
2. Implement `next` from the plan
  a. Write only what this step calls for
  b. No speculative abstractions, no changes to unrelated code
  c. Match the style and conventions already in the file
3. Run whatever checks confirm the code is valid for this project
  a. Compiled languages: build
  b. Interpreted languages: lint or type-check
  c. If there are errors, fix them before moving on
