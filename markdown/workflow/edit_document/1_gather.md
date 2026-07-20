## Purpose
Collect relevant information to inform documentation goal and structure

## Process
1. Identify documentation target:
   - User-specified file path → use that
   - User request without path → use list_files to search for README.md, docs/, or *.md files; if multiple found, list them and ask user to specify
   - Create new → store target location in target_file field
2. Gather context:
   - Existing documentation in target file (if updating)
   - Related source code, APIs, configurations
   - Documentation in same directory or parent docs/ folder
3. Identify purpose from:
   - User's explicit request
   - File location conventions (README = project overview, docs/ = detailed guides)

## Workflow Step Artifact
```yaml
target_file: "path/to/file.md"
action: "create | update"
context:
  existing_content: "current file content for update operations; null for create operations"
  references:
    - path: "file/path"
      relevance: "what information this file provides to the document"
  purpose: "what the documentation should achieve"
```
