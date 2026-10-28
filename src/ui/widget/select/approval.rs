use std::sync::{Arc, Mutex};

use nvim_oxi::Result as OxiResult;
use tokio::sync::oneshot;

use super::SelectOption;
use super::SelectWidget;
use crate::ui::{nvim_primitives::buffer::NvimBuffer, widget::Widget};
use crate::utils::approval::{Approval, ApprovalResult, deny_message};

/// Sends an `ApprovalResult` through `response_tx`. First caller wins via
/// `Mutex<Option<...>>::take()`.
fn send_result(
    response_tx: &Arc<Mutex<Option<oneshot::Sender<ApprovalResult>>>>,
    approved: bool,
    reject_message: &str,
) {
    if let Ok(mut guard) = response_tx.lock()
        && let Some(tx) = guard.take()
    {
        let result = if approved {
            ApprovalResult::Approved
        } else {
            ApprovalResult::Rejected(deny_message(reject_message))
        };
        let _ = tx.send(result);
    }
}

/// Fires the completion signal so the waiting loop knows the approval is resolved.
fn signal_completion(completion: &Arc<Mutex<Option<oneshot::Sender<()>>>>) {
    if let Ok(mut guard) = completion.lock()
        && let Some(tx) = guard.take()
    {
        let _ = tx.send(());
    }
}

/// A tool-approval widget asking the user to approve or reject a tool call.
///
/// Shows the tool name and its YAML-rendered args, followed by Approve and
/// Reject options. `<c-c>` cancel counts as rejection.
#[derive(Clone)]
pub struct ApprovalWidget {
    select: SelectWidget,
}

impl ApprovalWidget {
    pub fn new(action: Approval, completion_tx: oneshot::Sender<()>) -> OxiResult<Self> {
        let Approval {
            tool,
            args,
            reject_message,
            response_tx,
        } = action;

        // Fall back to JSON rendering if YAML serialization fails.
        let yaml_args = serde_yaml::to_string(&args).unwrap_or_else(|_| args.to_string());
        let title = format!(
            "Agent is trying to call `{tool}` tool with the following args:\n```yaml\n{yaml_args}```"
        );

        let reject_text = format!("Reject: {reject_message}");
        let options = vec![
            SelectOption {
                text: "Approve".to_string(),
                recommended: false,
                free_text: false,
            },
            SelectOption {
                text: reject_text,
                recommended: false,
                free_text: false,
            },
        ];

        let shared_completion = Arc::new(Mutex::new(Some(completion_tx)));

        let select_resp_tx = Arc::clone(&response_tx);
        let select_comp = Arc::clone(&shared_completion);
        let select_reject = reject_message.clone();
        let on_select: Option<Box<dyn FnMut(usize) + Send + Sync>> = Some(Box::new(move |idx| {
            send_result(&select_resp_tx, idx == 0, &select_reject);
            signal_completion(&select_comp);
        }));

        let cancel_resp_tx = Arc::clone(&response_tx);
        let cancel_comp = Arc::clone(&shared_completion);
        let cancel_reject = reject_message.clone();
        let on_cancel: Option<Box<dyn FnOnce() + Send + Sync>> = Some(Box::new(move || {
            send_result(&cancel_resp_tx, false, &cancel_reject);
            signal_completion(&cancel_comp);
        }));

        let select = SelectWidget::new(&title, &options, on_select, on_cancel)?;

        Ok(Self { select })
    }
}

impl Widget for ApprovalWidget {
    fn render(&mut self) -> OxiResult<()> {
        self.select.render()
    }

    fn buffer(&self) -> &NvimBuffer {
        self.select.buffer()
    }
}
