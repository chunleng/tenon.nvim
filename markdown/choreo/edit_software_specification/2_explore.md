## Process
1. Determine exploration from `decision_frame.mode` in memory:
   - For updates, navigate to the next move without generating alternatives. Preserve existing alternatives without requiring regeneration
   - For creation, ask the user whether alternatives are needed and wait for their answer. Recommend based on the decision's complexity and stakes
   - Generate alternatives only if the user opts in. If they decline, navigate to the next move
2. Generate viable alternatives:
   - Include "do nothing" or "status quo" as a baseline and at least one other viable alternative
   - Present options honestly; a recommendation can come later
   - For each alternative, identify trade-offs (gains and costs), risks (what could go wrong), and assumptions (what must be true)
3. Scale depth to the decision's complexity: simpler, lower-stakes decisions warrant fewer alternatives and briefer analysis; high-stakes or hard-to-reverse decisions warrant more alternatives and deeper analysis
4. Present generated analysis to the user for confirmation. Incorporate feedback and repeat from process step 1 until confirmed

## Choreo Move Artifact
Include this artifact only when alternatives were considered and confirmed. For updates or when the user declines alternatives, omit the artifact.
```yaml
alternatives:
  - name: Option name
    trade_offs: Gains and costs
    risks: What could go wrong
    assumptions: What must be true
```
