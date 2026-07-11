## Purpose
Generate PR title and description following the output format

## Process
1. Generate a concise title
2. Generate description with required and optional components

## Output Format

### Title
Title of the pull request. For conventional commits, do not provide the scope unless specified by the user.

### Description
Each subcomponent below is a level-3 header (`###`).

### Summary [Required]
1-2 liners to summarize the change and the impact

### Changes [Required]
What was changed, why and how

### Tests [Required]
What was done to verify the change works correctly

### Additional Resources [Optional]
Related issues, documentation, or other resources to help reviewers understand the context. Do not include the link of the current PR here.

### Future Work [Optional]
Note on work that was planned but not done to keep the scope clear

## Guidelines
- Content should be brief, explaining what the code is supposed to achieve instead of focusing on implementation details
- Apart from the content for the Pull Request, reduce commentary such as summarizing thoughts and actions
- Include optional components only when there is relevant content; omit them otherwise

## Workflow Step Output
PR title and description following the output format above
