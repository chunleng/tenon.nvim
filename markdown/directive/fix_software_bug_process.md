## Workflow

Follow procedure, step by step. Never skip ahead. Announce before starting each step and after completing each step

### 1. Reproduce

- Create minimal test case triggering bug. Techniques by preference; combine as needed:
    * Print/Telemetry: add log statements to trace execution path + variable values
    * Binary Search: comment out half the code; bug disappears → cause in removed half; repeat until narrowed to line
    * Fuzzing: feed random/mutated inputs to discover crashes or unexpected behavior; especially for parsers/decoders
    * Mock/Stub: replace external deps (API, DB, filesystem) with controllable doubles; force edge cases, eliminate real-dep flakiness
    * Isolation: extract suspicious code into standalone test/project; remove deps + unrelated logic until bug pinned down or disappears (disappearance → removed dep is cause)
    * Visual Diff: UI bugs → capture + diff screenshots of expected vs actual render
- Test requires user confirmation (live service, destructive operation, human-validated UI) → prefer alternative technique agent can run autonomously; none available → ask user to confirm
- **Never proceed without reproduction.** Diagnosing or fixing a bug you cannot reproduce is strictly prohibited — you will guess, and guesses are wrong. Cannot reproduce → state this, ask for more info, stop
- Record: input, environment state, error/wrong output

**Yields**: reproduction test case + recorded details

### 2. Understand

- Read error message in full → what failed, where, expected vs actual
- State expected + actual behavior before reading code
- Expected behavior unclear → ask, never assume

**Yields**: expected vs actual behavior statement

### 3. Locate

- Start from error surface (stack trace, failing test, reported location)
- Trace backward to root cause → never stop at symptom
- Multiple possible causes → prefer simplest one explaining all observations
- Identify specific line(s)/function(s) to change

**Yields**: root cause location

### 4. Fix

- Make minimal change resolving root cause
- Never refactor, optimize, or improve unrelated code
- Minimal fix insufficient → explain why before expanding scope
- One fix per bug. Second bug found → address separately after

**Yields**: minimal code change

### 5. Verify

- Run reproduction case → confirm bug resolved
- Run existing tests → confirm no regressions
- No test covers fixed code path → write one
- Verification fails → return to step 3. Never patch on top of a broken fix
- Verification fails repeatedly (3+ attempts) → root cause likely misidentified. Before next fix attempt:
    * Re-examine step 2: is "expected vs actual" statement correct?
    * Re-examine step 3: is identified root cause a symptom, not the cause?
    * Code too complex to reason about → refactor for clarity, then re-locate
    * Logic requires excessive special cases → different data structure or algorithm
    * Multiple interacting bugs → fix one at a time; second bug may mask the first

**Yields**: all tests passing

### 6. Clean Up

- Remove all debug output, temporary variables, commented-out code added during investigation
- No leftover debugging artifacts

**Yields**: clean, commit-ready state
