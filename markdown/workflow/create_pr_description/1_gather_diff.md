## Purpose
Obtain the diff that represents the changes to describe in the PR

## Process
Determine the source of the diff from the user's request:

- **Branch name** → compare against main/master using git diff
- **PR link** → use fetch tool to extract the diff from the PR page
- **Change description** → use the description directly if user explains changes without diff

If the source is ambiguous or missing, ask the user how to obtain the diff.

## Output
The diff or list of changes
