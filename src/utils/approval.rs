use crate::chat::{EventChannel, PendingAction};
use rig::tool::ToolContext;
use std::sync::{Arc, Mutex, Weak};

/// A tool requesting user approval before performing an action.
#[derive(Clone)]
pub struct Approval {
    pub tool: String,
    pub args: serde_json::Value,
    pub reject_message: String,
    pub response_tx: Arc<Mutex<Option<tokio::sync::oneshot::Sender<ApprovalResult>>>>,
}

/// Result of an approval action.
#[derive(Debug, PartialEq)]
pub enum ApprovalResult {
    Approved,
    Rejected(String),
}

/// Builds the deny message returned to the tool when the user rejects via the widget: the
/// `::tenon-denied::` prefix is a Tenon prompt
pub fn deny_message(reject_message: &str) -> String {
    format!("::tenon-denied:: {reject_message}")
}

/// Request user approval for a tool action. Pushes an Approval pending action to the session's
/// pending actions channel and awaits the user's verdict. An unavailable channel (missing or dead)
/// or a dropped response defaults to reject. Returns `Approved` if the user approved, or `Rejected`
/// with the message the calling tool should surface to the agent.
pub async fn request_approval(
    context: &mut ToolContext,
    tool: &str,
    args: serde_json::Value,
    reject_message: String,
) -> ApprovalResult {
    let Some(event_channel) = context
        .scope::<Weak<EventChannel<PendingAction>>>()
        .and_then(|weak| weak.upgrade())
    else {
        return ApprovalResult::Rejected(reject_message);
    };

    let (tx, rx) = tokio::sync::oneshot::channel::<ApprovalResult>();
    event_channel.push(PendingAction::Approval(Approval {
        tool: tool.to_string(),
        args,
        reject_message: reject_message.clone(),
        response_tx: Arc::new(Mutex::new(Some(tx))),
    }));

    match rx.await {
        Ok(result) => result,
        Err(_) => ApprovalResult::Rejected(reject_message),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn new_channel() -> Arc<EventChannel<PendingAction>> {
        Arc::new(EventChannel::new())
    }

    /// Spawns `request_approval` with its own context bound to the channel,
    /// since the helper pushes and awaits within a single call.
    fn spawn_request(
        weak: Weak<EventChannel<PendingAction>>,
        tool: &'static str,
        args: serde_json::Value,
        reject_message: String,
    ) -> tokio::task::JoinHandle<ApprovalResult> {
        tokio::spawn(async move {
            let mut context = ToolContext::new().with_scope(Arc::new(weak));
            request_approval(&mut context, tool, args, reject_message).await
        })
    }

    /// Waits until the front pending action is an Approval and returns it.
    async fn wait_for_approval(channel: &EventChannel<PendingAction>) -> PendingAction {
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            loop {
                if let Some(action) = channel.peek() {
                    return action;
                }
                tokio::time::sleep(std::time::Duration::from_millis(1)).await;
            }
        })
        .await
        .expect("approval action should be pushed")
    }

    /// Simulates the widget: on reject, sends the deny message computed from
    /// the action's own reject message, exactly as `send_result` does.
    fn respond(approval: Approval, approved: bool) {
        let tx = approval
            .response_tx
            .lock()
            .unwrap()
            .take()
            .expect("tx available");
        let result = if approved {
            ApprovalResult::Approved
        } else {
            ApprovalResult::Rejected(deny_message(&approval.reject_message))
        };
        tx.send(result).expect("send ok");
    }

    #[tokio::test]
    async fn test_request_approval_pushes_action_and_returns_verdict() {
        let channel = new_channel();
        let weak = Arc::downgrade(&channel);

        // Approve: pushed action carries tool name, args, and reject message
        let task = spawn_request(
            weak.clone(),
            "read_file",
            json!({"filepath": ".env"}),
            "attempt to read sensitive path".to_string(),
        );
        let PendingAction::Approval(approval) = wait_for_approval(&channel).await else {
            panic!("front pending action should be an approval");
        };
        assert_eq!(approval.tool, "read_file");
        assert_eq!(approval.args, json!({"filepath": ".env"}));
        assert_eq!(approval.reject_message, "attempt to read sensitive path");
        respond(approval, true);
        assert_eq!(
            task.await.expect("task should not panic"),
            ApprovalResult::Approved
        );
        channel.mark_done();

        // Reject: reject message is prefixed and returned to the caller
        let task = spawn_request(
            weak.clone(),
            "read_file",
            json!({}),
            "attempt to read sensitive path".to_string(),
        );
        let PendingAction::Approval(approval) = wait_for_approval(&channel).await else {
            panic!("front pending action should be an approval");
        };
        respond(approval, false);
        assert_eq!(
            task.await.expect("task should not panic"),
            ApprovalResult::Rejected("::tenon-denied:: attempt to read sensitive path".to_string())
        );
        channel.mark_done();

        // Dropped response (widget gone without answering) defaults to reject
        // with the plain reject message, without the deny prefix
        let task = spawn_request(weak, "read_file", json!({}), "no answer".to_string());
        let PendingAction::Approval(approval) = wait_for_approval(&channel).await else {
            panic!("front pending action should be an approval");
        };
        drop(
            approval
                .response_tx
                .lock()
                .unwrap()
                .take()
                .expect("tx available"),
        );
        assert_eq!(
            task.await.expect("task should not panic"),
            ApprovalResult::Rejected("no answer".to_string())
        );
    }

    #[tokio::test]
    async fn test_request_approval_unavailable_channel_rejects() {
        // Channel missing from context entirely
        let mut context = ToolContext::new();
        assert_eq!(
            request_approval(
                &mut context,
                "read_file",
                json!({}),
                "attempt to read sensitive path".to_string()
            )
            .await,
            ApprovalResult::Rejected("attempt to read sensitive path".to_string()),
            "missing channel must default to reject"
        );

        // Channel present but dead (weak ref cannot upgrade)
        let channel = new_channel();
        let weak = Arc::downgrade(&channel);
        drop(channel);
        let mut context = ToolContext::new().with_scope(Arc::new(weak));
        assert_eq!(
            request_approval(
                &mut context,
                "read_file",
                json!({}),
                "attempt to read sensitive path".to_string()
            )
            .await,
            ApprovalResult::Rejected("attempt to read sensitive path".to_string()),
            "dead channel must default to reject"
        );
    }
}
