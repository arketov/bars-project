//! Сброс правок окна перед выходом (НФТ-5): процесс просит окно записать документы и ждёт ответа.
//!
//! Окно отвечает командой `flush_reply`: `clean` — писать нечего или всё записано; `asking` — остались
//! документы, которые не записались (конфликт, ошибка), окно спрашивает пользователя; `proceed` — пользователь
//! согласился закрыть без записи; `cancel` — остаться.

use std::sync::{Mutex, PoisonError};
use std::time::Duration;

use tauri::{AppHandle, Emitter, Manager};
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};

use crate::commands::Shared;

/// Сколько ждать первого ответа окна: окна может не быть или оно ещё не загрузилось.
const QUICK: Duration = Duration::from_secs(3);
/// Сколько ждать решения пользователя в диалоге.
const PATIENT: Duration = Duration::from_secs(3600);

#[derive(Default)]
pub struct Flush {
    sender: Mutex<Option<UnboundedSender<String>>>,
}

impl Flush {
    fn begin(&self) -> UnboundedReceiver<String> {
        let (tx, rx) = unbounded_channel();
        *self.sender.lock().unwrap_or_else(PoisonError::into_inner) = Some(tx);
        rx
    }

    fn end(&self) {
        *self.sender.lock().unwrap_or_else(PoisonError::into_inner) = None;
    }

    /// Ответ окна; `false` — сейчас никто не ждёт.
    pub fn reply(&self, state: String) -> bool {
        self.sender
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .as_ref()
            .is_some_and(|sender| sender.send(state).is_ok())
    }
}

/// Ждёт ответ окна. `true` — можно выходить. Молчание окна за `quick` — можно (нечего сбрасывать или окно
/// не отвечает); молчание после вопроса пользователю за `patient` — нельзя.
async fn wait(
    rx: &mut UnboundedReceiver<String>,
    quick: Duration,
    patient: Duration,
    mut on_asking: impl FnMut(),
) -> bool {
    let mut asked = false;
    loop {
        let limit = if asked { patient } else { quick };
        match tokio::time::timeout(limit, rx.recv()).await {
            Ok(Some(state)) => match state.as_str() {
                "clean" | "proceed" => return true,
                "cancel" => return false,
                "asking" => {
                    asked = true;
                    on_asking();
                }
                _ => {}
            },
            Ok(None) => return !asked,
            Err(_) => return !asked,
        }
    }
}

/// Просит окно записать несохранённые документы. `false` — пользователь или ошибка записи просят остаться.
pub async fn request(app: &AppHandle) -> bool {
    let state = app.state::<Shared>();
    let mut rx = state.flush.begin();
    if app.emit("bars://flush-request", ()).is_err() {
        state.flush.end();
        return true;
    }
    let shown = app.clone();
    let proceed = wait(&mut rx, QUICK, PATIENT, move || {
        if let Some(window) = shown.get_webview_window("main") {
            let _ = window.show();
            let _ = window.unminimize();
            let _ = window.set_focus();
        }
    })
    .await;
    state.flush.end();
    proceed
}

#[cfg(test)]
mod tests {
    use super::*;

    const SHORT: Duration = Duration::from_millis(60);

    fn replies(states: &[&str]) -> UnboundedReceiver<String> {
        let (tx, rx) = unbounded_channel();
        for state in states {
            tx.send((*state).to_string()).unwrap();
        }
        // Отправитель живёт, пока жив тест: закрытый канал — отдельный случай.
        std::mem::forget(tx);
        rx
    }

    #[tokio::test]
    async fn clean_window_lets_the_process_exit() {
        assert!(wait(&mut replies(&["clean"]), SHORT, SHORT, || {}).await);
    }

    #[tokio::test]
    async fn silent_window_does_not_block_exit() {
        assert!(wait(&mut replies(&[]), SHORT, SHORT, || {}).await);
    }

    #[tokio::test]
    async fn user_can_discard_or_stay_after_the_window_asked() {
        let mut asked = 0;
        assert!(
            wait(&mut replies(&["asking", "proceed"]), SHORT, SHORT, || {
                asked += 1
            })
            .await
        );
        assert_eq!(asked, 1);
        assert!(!wait(&mut replies(&["asking", "cancel"]), SHORT, SHORT, || {}).await);
    }

    #[tokio::test]
    async fn unanswered_question_keeps_the_process_running() {
        assert!(!wait(&mut replies(&["asking"]), SHORT, SHORT, || {}).await);
    }

    #[tokio::test]
    async fn unknown_replies_are_ignored() {
        assert!(wait(&mut replies(&["???", "clean"]), SHORT, SHORT, || {}).await);
    }

    #[test]
    fn reply_reaches_only_the_active_request() {
        let flush = Flush::default();
        assert!(!flush.reply("clean".into()));
        let mut rx = flush.begin();
        assert!(flush.reply("clean".into()));
        assert_eq!(rx.try_recv().unwrap(), "clean");
        flush.end();
        assert!(!flush.reply("clean".into()));
    }
}
