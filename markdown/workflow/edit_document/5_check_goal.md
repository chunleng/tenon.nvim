## Purpose
Verify documentation meets the goal defined in Set Goal step

## Process
1. Read the target file
2. Check against goal:
   - Does it cover the defined scope?
   - Are all success criteria met?
3. Determine result:
   - Goal achieved → workflow ends
   - Goal not achieved → identify gaps, redirect to Execute

## Output
- Goal achieved → `null` (workflow ends)
- Goal not achieved → `gaps: ["criteria not met and what's missing"]` (redirect to Execute)
