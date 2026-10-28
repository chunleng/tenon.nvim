use std::sync::{Arc, Mutex};

use nvim_oxi::{Result as OxiResult, api::types::LogLevel};
use tokio::sync::oneshot;

use super::{SelectOption, SelectWidget};
use crate::tools::ask_question::{Question, QuestionResult};
use crate::ui::{
    nvim_primitives::buffer::NvimBuffer,
    nvim_primitives::window::NvimWindow,
    widget::Widget,
    widget::free_text::{FreeTextWidget, FreeTextWidgetOption},
};
use crate::{get_chat_window, utils::notify};

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

/// Pushes a FreeTextWidget on top of the input panel's widget stack so the user can type an answer.
fn open_free_text_input(
    question: &str,
    chat_key: &str,
    response_tx: Arc<Mutex<Option<oneshot::Sender<QuestionResult>>>>,
    completion: Arc<Mutex<Option<oneshot::Sender<()>>>>,
) {
    let submit_resp_tx = Arc::clone(&response_tx);
    let submit_comp = Arc::clone(&completion);
    let on_submit: Option<Box<dyn FnMut(String) + Send + Sync>> = Some(Box::new(move |text| {
        send_result(&submit_resp_tx, Some(text));
        signal_completion(&submit_comp);
    }));

    let cancel_chat_key = chat_key.to_string();
    let on_cancel: Option<Box<dyn FnOnce() + Send + Sync>> = Some(Box::new(move || {
        // Pop only the free text widget: the question widget below stays
        // active, so the user can pick another option. The question is not
        // resolved here, so no result or completion is sent.
        let result: OxiResult<()> = (|| {
            nvim_oxi::api::command("stopinsert")?;
            get_chat_window()
                .lock()
                .map_err(|_| {
                    nvim_oxi::Error::Mlua(mlua::Error::RuntimeError("poisoned lock".into()))
                })?
                .pop_input_widget(&cancel_chat_key)?;
            Ok(())
        })();
        if let Err(err) = result {
            notify(
                format!("Failed to close the free text input: {err}"),
                LogLevel::Error,
            );
        }
    }));

    let result: OxiResult<()> = (|| {
        let widget = FreeTextWidget::new(
            question,
            FreeTextWidgetOption::default(),
            on_submit,
            on_cancel,
        )?;
        get_chat_window()
            .lock()
            .map_err(|_| nvim_oxi::Error::Mlua(mlua::Error::RuntimeError("poisoned lock".into())))?
            .push_input_widget(chat_key, Box::new(widget))?;
        Ok(())
    })();

    if let Err(err) = result {
        notify(
            format!("Failed to open the free text input: {err}"),
            LogLevel::Error,
        );
    }
}

/// Collapses options with duplicate texts. Winner per text: free-text beats everything, then
/// recommended beats non-recommended, then first occurrence wins. The result is ordered recommended
/// first, then non-recommended, then free-text, preserving the original order within each tier.
fn dedupe_options(options: Vec<SelectOption>) -> Vec<SelectOption> {
    let mut result: Vec<SelectOption> = Vec::new();
    for opt in options {
        if let Some(existing) = result.iter_mut().find(|o| o.text == opt.text) {
            if opt.free_text || (!existing.free_text && opt.recommended && !existing.recommended) {
                *existing = opt;
            }
        } else {
            result.push(opt);
        }
    }
    // A dedupe winner can inherit a position from a higher tier (e.g. a free-text option beating a
    // recommended duplicate at the top), so re-sort into tiers. Stable, so within-tier order is
    // preserved.
    result.sort_by_key(|opt| (opt.free_text, std::cmp::Reverse(opt.recommended)));
    result
}

/// Inner presentation of a `QuestionWidget`: a select list when options are
/// provided, or a direct free text input for free-form questions.
#[derive(Clone)]
enum QuestionInner {
    Select(SelectWidget),
    FreeText(FreeTextWidget),
}

/// A question-answering widget displaying a title and selectable options.
///
/// Each option carries its own handler, invoked when the user selects it.
/// A cancel handler is invoked when the user cancels via `<c-c>` or presses
/// `<cr>` outside an option line.
#[derive(Clone)]
pub struct QuestionWidget {
    inner: QuestionInner,
}

impl QuestionWidget {
    pub fn new(
        action: Question,
        chat_key: &str,
        completion_tx: oneshot::Sender<()>,
    ) -> OxiResult<Self> {
        let Question {
            question,
            open_ended_text,
            options,
            response_tx,
        } = action;

        let shared_completion = Arc::new(Mutex::new(Some(completion_tx)));

        // Use free text directly when there's no options
        if options.is_empty() {
            let submit_resp_tx = Arc::clone(&response_tx);
            let submit_comp = Arc::clone(&shared_completion);
            let on_submit: Option<Box<dyn FnMut(String) + Send + Sync>> =
                Some(Box::new(move |text| {
                    send_result(&submit_resp_tx, Some(text));
                    signal_completion(&submit_comp);
                }));

            let cancel_resp_tx = Arc::clone(&response_tx);
            let cancel_comp = Arc::clone(&shared_completion);
            let on_cancel: Option<Box<dyn FnOnce() + Send + Sync>> = Some(Box::new(move || {
                send_result(&cancel_resp_tx, None);
                signal_completion(&cancel_comp);
            }));

            let widget = FreeTextWidget::new(
                &question,
                FreeTextWidgetOption::default(),
                on_submit,
                on_cancel,
            )?;
            return Ok(Self {
                inner: QuestionInner::FreeText(widget),
            });
        }

        let mut all_options = options;
        all_options.push(SelectOption {
            text: open_ended_text.unwrap_or_else(|| "Type your answer..".to_string()),
            recommended: false,
            free_text: true,
        });
        let all_options = dedupe_options(all_options);

        let handlers: Vec<Option<Box<dyn Fn() + Send + Sync>>> = all_options
            .iter()
            .map(|opt| {
                if opt.free_text {
                    let resp_tx = Arc::clone(&response_tx);
                    let comp = Arc::clone(&shared_completion);
                    let question = question.clone();
                    let chat_key = chat_key.to_string();
                    return Some(Box::new(move || {
                        open_free_text_input(
                            &question,
                            &chat_key,
                            Arc::clone(&resp_tx),
                            Arc::clone(&comp),
                        );
                    }) as Box<dyn Fn() + Send + Sync>);
                }
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

        let select = SelectWidget::new(&question, &all_options, on_select, on_cancel)?;

        Ok(Self {
            inner: QuestionInner::Select(select),
        })
    }
}

impl Widget for QuestionWidget {
    fn render(&mut self) -> OxiResult<()> {
        match &mut self.inner {
            QuestionInner::Select(select) => select.render(),
            QuestionInner::FreeText(free_text) => free_text.render(),
        }
    }

    fn buffer(&self) -> &NvimBuffer {
        match &self.inner {
            QuestionInner::Select(select) => select.buffer(),
            QuestionInner::FreeText(free_text) => free_text.buffer(),
        }
    }

    fn set_window(&mut self, window: NvimWindow) {
        match &mut self.inner {
            QuestionInner::Select(select) => select.set_window(window),
            QuestionInner::FreeText(free_text) => free_text.set_window(window),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn opt(text: &str, recommended: bool) -> SelectOption {
        SelectOption {
            text: text.to_string(),
            recommended,
            free_text: false,
        }
    }

    fn free_opt(text: &str) -> SelectOption {
        SelectOption {
            text: text.to_string(),
            recommended: false,
            free_text: true,
        }
    }

    /// Covers all dedupe priorities in one scenario chain:
    /// free-text beats recommended duplicate, recommended beats
    /// non-recommended duplicate, first occurrence wins among same tier,
    /// and the winner is placed in its own tier (free-text last even when
    /// it beat a recommended duplicate at the top).
    #[test]
    fn test_dedupe_options_priority() {
        let options = vec![
            opt("Type your answer..", true), // free-text wins, but demoted to free-text tier
            opt("Approve", false),
            opt("Approve", true), // recommended beats first occurrence
            opt("Decline", true),
            opt("Decline", true), // first occurrence wins among recommended
            free_opt("Type your answer.."),
        ];

        let result = dedupe_options(options);

        let texts: Vec<&str> = result.iter().map(|o| o.text.as_str()).collect();
        assert_eq!(texts, vec!["Approve", "Decline", "Type your answer.."]);
        assert!(result[0].recommended);
        assert!(result[1].recommended);
        assert!(result[2].free_text);
    }
}
