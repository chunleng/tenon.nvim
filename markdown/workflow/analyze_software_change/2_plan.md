## Process
1. If re-entered from Validate, note the existing plan in workflow memory and user feedback.
2. Plan feature steps. Each feature step must satisfy all three criteria:
   - Can be partial — the step's deliverable can be a partially working feature or use stubs/placeholder data. It does not need to be a complete, fully functional feature.
   - Must not break the system — the system remains functional after each step
   - User-visible — the user can run the system and see the step's deliverable
3. Evaluate whether setup steps are needed, based on two triggers:
   - Features being built — does any feature require a test type or test framework not already in place? (e.g., a feature involving async messaging → introduce an integration test harness)
   - Non-functional requirements — does any requirement need a supporting framework to verify or sustain it? (e.g., high QPS → introduce a stress test framework)
4. Setup steps are exempt from the user-visible criterion, but must satisfy:
   - Justified — name the specific feature or non-functional requirement that requires it
   - Just-in-time — placed immediately before the first feature step that needs it. Do not batch setup steps together upfront.

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

### Setup Step Placement

- Good:
  - [setup] Introduce stress test framework — placed right before the feature step that must handle high QPS
  - [setup] Add integration test harness for message queue — placed right before the feature step that sends async notifications
- Bad:
  - [setup] Set up stress test framework as the first plan step — nothing uses it until much later
  - [setup] Add a test framework "just in case" — no concrete feature or requirement needs it yet

## Workflow Step Artifact
Provide the complete plan. Never provide partial updates — always include every step even if only one changed.
```yaml
steps:
  - type: setup
    step: "description"
    justification: "the concrete feature or non-functional requirement that requires this"
  - type: feature
    step: "description of what the step does"
    deliverable: "what the user sees when they run the system"
```

## Example
```yaml
steps:
  - type: feature
    step: "Display order status change notification with dummy data"
    deliverable: "User triggers a status change and sees an email notification (with placeholder content)"
  - type: setup
    step: "Introduce stress test framework"
    justification: "Needed to verify the high-QPS requirement before the next step sends real notifications at scale"
  - type: feature
    step: "Send real order details in notification at production volume"
    deliverable: "User triggers status change and receives email with actual order details; stress test confirms QPS target is met"
  - type: feature
    step: "Add notification preference toggle"
    deliverable: "User toggles preferences in settings and notifications respect the choice"
```
