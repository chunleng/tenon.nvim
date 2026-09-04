## Prompt Types

#### Constitution (Tenon Concept)
- Highest-priority governing rules. Overrides everything; nothing can contradict it.
- NEVER suggest editing outside the Tenon repository.
- Use when:
    - Establish non-negotiable boundaries the agent must never cross
    - Set decision resolution rules for conflicting instructions

#### Directive (Tenon Concept)
- Agent conduct rules with optional conditions
- Use when:
    - Alter agent behavior
    - Introduce knowledge

### Tool Definitions
Schema and instructions for tool/function calling.

- Tool name and description
- Parameter schema (types, required/optional)
- Usage instructions (when to call, how to handle results)
- Use when:
    - Provide new system interaction capabilities

### Choreo (Tenon Concept)
Multi-step process. Each move yields artifact for next. Injected context focuses attention per move, preventing sidetracking. Best for long-running instructions.

How it works:
- use_choreo to enter, end_choreo to exit, navigate_choreo to switch moves
- Instructions injected only when reaching that move, not stored in history. Keeps attention focused, avoids cluttering conversation.
- Use when:
    - Long-running instructions that risk sidetracking

## Techniques

### Injected Context
Add information to `context` tag in latest user prompt. Gets the most attention — use for highest-priority information.

### Use Harness to Show/Hide Information
Harness can show or hide information as needed.

Example: choreo reveals only the current move's instructions, hiding others until needed — like guiding the AI step by step.
