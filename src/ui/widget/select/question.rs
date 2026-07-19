use std::sync::{Arc, Mutex};

use nvim_oxi::Result as OxiResult;
use tokio::sync::oneshot;

use super::SelectWidget;
use crate::chat::PendingAction;
use crate::tools::ask_question::{ANSWER_BY_CHAT, QuestionResult};
use crate::ui::{
    nvim_primitives::buffer::{NvimBuffer, NvimKeymap},
    widget::Widget,
};

/// A selectable option with an associated handler fired on selection.
struct QuestionOption {
    text: String,
    handler: Box<dyn FnOnce() + Send + Sync>,
}

/// Sends a `QuestionResult` through `response_tx`. First caller wins via
/// `Mutex<Option<...>>::take()`.
pub(crate) fn send_result(
    response_tx: &Arc<Mutex<Option<oneshot::Sender<QuestionResult>>>>,
    response: Option<String>,
) {
    if let Ok(mut guard) = response_tx.lock()
        && let Some(tx) = guard.take()
    {
        let _ = tx.send(QuestionResult { response });
    }
}

/// Fires the completion signal so the waiting loop knows the question is resolved.
fn signal_completion(completion: &Arc<Mutex<Option<oneshot::Sender<()>>>>) {
    if let Ok(mut guard) = completion.lock()
        && let Some(tx) = guard.take()
    {
        let _ = tx.send(());
    }
}

/// A question-answering widget displaying a title and selectable options.
///
/// Each option carries its own handler, invoked when the user selects it.
/// A cancel handler is invoked when the user cancels via `<c-c>` or presses
/// `<cr>` outside an option line.
#[derive(Clone)]
pub struct QuestionWidget {
    select: SelectWidget,
}

impl QuestionWidget {
    pub fn new(
        action: PendingAction,
        completion_tx: oneshot::Sender<()>,
        base_keymaps: Vec<NvimKeymap>,
    ) -> OxiResult<Self> {
        let PendingAction::Question {
            question,
            options,
            response_tx,
        } = action;

        let shared_completion = Arc::new(Mutex::new(Some(completion_tx)));

        let mut all_option_texts = options;
        all_option_texts.push(ANSWER_BY_CHAT.to_string());

        let question_options: Vec<QuestionOption> = all_option_texts
            .into_iter()
            .map(|opt| {
                let resp_tx = Arc::clone(&response_tx);
                let comp = Arc::clone(&shared_completion);
                let opt_text = opt.clone();

                QuestionOption {
                    text: opt,
                    handler: Box::new(move || {
                        send_result(&resp_tx, Some(opt_text));
                        signal_completion(&comp);
                    }),
                }
            })
            .collect();

        let cancel_resp_tx = Arc::clone(&response_tx);
        let cancel_comp = Arc::clone(&shared_completion);
        let on_cancel: Option<Box<dyn FnOnce() + Send + Sync>> = Some(Box::new(move || {
            send_result(&cancel_resp_tx, None);
            signal_completion(&cancel_comp);
        }));

        let texts: Vec<String> = question_options.iter().map(|o| o.text.clone()).collect();
        let handlers: Vec<Option<Box<dyn FnOnce() + Send + Sync>>> = question_options
            .into_iter()
            .map(|o| Some(o.handler))
            .collect();

        let on_select: Option<Box<dyn FnOnce(usize) + Send + Sync>> = Some(Box::new(move |idx| {
            if let Some(handler) = handlers.into_iter().nth(idx).flatten() {
                handler();
            }
        }));

        let select = SelectWidget::new(&question, &texts, on_select, on_cancel, base_keymaps)?;

        Ok(Self { select })
    }
}

impl Widget for QuestionWidget {
    fn render(&mut self) -> OxiResult<()> {
        self.select.render()
    }

    fn buffer(&self) -> &NvimBuffer {
        self.select.buffer()
    }
}
