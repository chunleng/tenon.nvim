## Purpose
Gather requirements, identify existing features, detect contradictions with current behavior

## Process
1. If this is a revisit (issues returned from Verify step), address the issues first
2. Understand user's request:
  - high-level requests:
    - Grill me on plan (one question at a time) until shared understanding
    - Decompose → sub-features
    - Continue grill + decompose until sub-features clear
  - Ask clarifying questions for each component
    - If this is a greenfield project, always ask if test should be set up
3. Investigate codebase:
  - What existing features relate to this request
  - What behaviors might conflict or contradict
  - What assumptions exist in current implementation
4. Things to note for investigation:
  - What must stay the same
  - What can change
  - Migration requirements (for behavior changes)
  - Backward compatibility needs
  - Potential conflicts with existing features
  - Non-functional requirements (for significant changes):
    - Performance: response time, throughput, latency
    - Privacy: data handling, logging, PII
    - Security: authentication, authorization
    - Scalability: expected load, growth
5. Clarify conflicts and ambiguities with user

## Workflow Step Output
```yaml
requirement: "what user wants to achieve"
constraints:
  - "must preserve X behavior"
  - "cannot change Y interface"
  - "must support Z migration path"
acceptance_criteria:
  - criteria: "measurable outcome"
    verification: "how to verify"
```

## Example
```yaml
requirement: "add rate limiting to API endpoints"
constraints:
  - "must not break existing API clients"
  - "must be configurable per endpoint"
  - "must log rejected requests"
acceptance_criteria:
  - criteria: "requests beyond limit return 429"
    verification: "test with burst of requests"
```
