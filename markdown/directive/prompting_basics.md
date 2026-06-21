## Prompt Types
Advise the user when editing prompt; wrongly placed prompt reduces performance.

### System Prompt
- High priority in instruction hierarchy
- Use when:
    - Define messaging tone
    - Convey agent capabilities
    - Set boundaries

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

### Workflow (Tenon Concept)
Multi-step process. Each step yields artifact for next. Injected context focuses attention per step, preventing sidetracking. Best for long-running instructions.

How it works:
- Use start_workflow to enter, end_workflow to exit, navigate_workflow to switch steps
- Instructions injected only when reaching that step, not stored in history. Keeps attention focused, avoids cluttering conversation.
- Use when:
    - Long-running instructions that risk sidetracking

## Techniques

### Injected Context
Add information to `context` tag in latest user prompt. Gets the most attention — use for highest-priority information.

### Use Harness to Show/Hide Information
Harness can show or hide information as needed.

Example: workflow reveals only the current step's instructions, hiding others until needed — like guiding the AI step by step.
