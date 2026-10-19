use std::sync::{Arc, Mutex};

use nvim_oxi::Result as OxiResult;
use tokio::sync::oneshot;

use super::SelectWidget;
use crate::tools::ask_question::{ANSWER_BY_CHAT, AskQuestionOption, Question, QuestionResult};
use crate::ui::{
    nvim_primitives::buffer::{NvimBuffer, NvimKeymap},
    widget::Widget,
};

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
        action: Question,
        completion_tx: oneshot::Sender<()>,
        base_keymaps: Vec<NvimKeymap>,
    ) -> OxiResult<Self> {
        let Question {
            question,
            options,
            response_tx,
        } = action;

        let shared_completion = Arc::new(Mutex::new(Some(completion_tx)));

        let mut all_options = options;
        all_options.push(AskQuestionOption {
            text: ANSWER_BY_CHAT.to_string(),
            recommended: false,
        });

        let handlers: Vec<Option<Box<dyn Fn() + Send + Sync>>> = all_options
            .iter()
            .map(|opt| {
                let resp_tx = Arc::clone(&response_tx);
                let comp = Arc::clone(&shared_completion);
                let text = opt.text.clone();
                Some(Box::new(move || {
                    send_result(&resp_tx, Some(text.clone()));
                    signal_completion(&comp);
                }) as Box<dyn Fn() + Send + Sync>)
            })
            .collect();

        let cancel_resp_tx = Arc::clone(&response_tx);
        let cancel_comp = Arc::clone(&shared_completion);
        let on_cancel: Option<Box<dyn FnOnce() + Send + Sync>> = Some(Box::new(move || {
            send_result(&cancel_resp_tx, None);
            signal_completion(&cancel_comp);
        }));

        let on_select: Option<Box<dyn FnMut(usize) + Send + Sync>> = Some(Box::new(move |idx| {
            if let Some(Some(handler)) = handlers.get(idx) {
                handler();
            }
        }));

        let select =
            SelectWidget::new(&question, &all_options, on_select, on_cancel, base_keymaps)?;

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
