## Purpose
Verify documentation meets the goal defined in Set Goal move

## Process
1. Read the target file
2. Check against goal:
   - Does it cover the defined scope?
   - Are all success criteria met?
3. Determine result:
   - Goal achieved → choreo ends
   - Goal not achieved → identify gaps, redirect to Execute

## Choreo Move Artifact
- Goal achieved → `null` (choreo ends)
- Goal not achieved → `gaps: ["criteria not met and what's missing"]` (redirect to Execute)
