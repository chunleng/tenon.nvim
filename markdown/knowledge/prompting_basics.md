Prompt engineering = context control. Right information at right time → significant result difference. Bad context → contradicts understanding → wrong decisions.

## Prompt Types

### System Prompt

- High priority in instruction hierarchy
- Use when:
    - Define messaging tone
    - Convey agent capabilities
    - Set boundaries
    - Add Tenon instructions

#### Behavior & Knowledge (Tenon Concept)

- Behavior: Scoping mechanism with optional conditions
- Knowledge: Information blocks inside behavior
- Both = prompt components, defined at program start
- Use when:
    - Reusable instructions shared across agents
    - Conditional execution

### Tool Definitions

Schema and instructions for tool/function calling.

- Tool name and description
- Parameter schema (types, required/optional)
- Usage instructions (when to call, how to handle results)

**How it works**: Model receives tool schema alongside prompt. Outputs structured tool calls when task requires.
- Use when:
    - Provide new system interaction capabilities
    - Reveal information at certain points (e.g., workflow tool controls instruction release)

### Workflow (Tenon Concept)

Multi-step process. Each step yields artifact for next. Uses injected context to focus LLM attention per step, constant reminders prevent sidetracking. Recommended for long-running instructions.

How it works:
- Use start_workflow to enter, end_workflow to exit, navigate_workflow to switch steps
- Instructions injected only when reaching that step, not stored in history. Keeps attention focused, avoids cluttering conversation.

## Instruction Priority
```
Latest User Prompt > Earlier User/Tool Prompt > System Prompt (includes Behavior & Knowledge)
```

Inside Each Type:

- **Latest User Prompt**: Later parts matter more → later instructions clarify or override earlier in same prompt
- **Earlier User/Tool Prompt**: Later messages matter more → most recent messages reflect current intent
- **System Prompt**: Earlier parts matter more → model attends first, establishes baseline

## Techniques

### Injected Context

Information needing highest priority → place in latest user prompt to ensure agent attention.

### Context via Program Flow Control

Instead of placing information in mundane locations, improve chat processing procedure. Workflow example: instructions appear only when needed, removed when irrelevant. Requires state management in chat flow.
