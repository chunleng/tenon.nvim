## Process
1. If re-entered from Validate, note the existing plan in workflow memory and user feedback.
2. Each step must satisfy all three criteria:
   - Can be partial — the step's deliverable can be a partially working feature or use stubs/placeholder data. It does not need to be a complete, fully functional feature.
   - Must not break the system — the system remains functional after each step
   - User-visible — the user can run the system and see the step's deliverable

## Bad vs Good

### Describe Outcomes, Not Implementation

- Good: User receives a message and sees a notification appear on screen
- Bad: Create a WebSocket handler that listens for incoming events and renders a notification component in the DOM

### Horizontal Layer vs Vertical Slice

- Good:
  - Display a notification with dummy data — user runs the system and sees a notification with placeholder content
  - Show search results with hardcoded entries — user performs a search and sees a results page
  - Render a login form that accepts any credentials — user sees the form and can submit it
- Bad:
  - Create a formatting function for notifications — nothing for the user to run
  - Set up database schema for notifications — invisible plumbing
  - Write a data parser — no user-visible output until connected to a UI

## Workflow Step Output
Output the complete plan. Never output partial updates — always include every step even if only one changed.
```yaml
steps:
  - step: "description of what the step does"
    deliverable: "what the user sees when they run the system"
  - step: "description"
    deliverable: "what the user sees"
```

## Example
```yaml
steps:
  - step: "Display order status change notification with dummy data"
    deliverable: "User triggers a status change and sees an email notification (with placeholder content)"
  - step: "Send real order details in notification"
    deliverable: "User triggers status change and receives email with actual order details"
  - step: "Add notification preference toggle"
    deliverable: "User toggles preferences in settings and notifications respect the choice"
```
