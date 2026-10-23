//! Сброс правок окна перед выходом (НФТ-5): процесс просит окно записать документы и ждёт ответа.
//!
//! Окно отвечает командой `flush_reply`: `clean` — писать нечего или всё записано; `asking` — остались
//! документы, которые не записались (конфликт, ошибка), окно спрашивает пользователя; `proceed` — пользователь
//! согласился закрыть без записи; `cancel` — остаться.

use std::collections::HashSet;
use std::sync::{Mutex, PoisonError};
use std::time::Duration;

use tauri::{AppHandle, Emitter, Manager};
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};

use crate::commands::Shared;

/// Сколько ждать первого ответа окна: окна может не быть или оно ещё не загрузилось.
const QUICK: Duration = Duration::from_secs(3);
/// Сколько ждать решения пользователя в диалоге.
const PATIENT: Duration = Duration::from_secs(3600);

/// Ответ окна: подпись окна и состояние.
type Reply = (String, String);

#[derive(Default)]
pub struct Flush {
    sender: Mutex<Option<UnboundedSender<Reply>>>,
    operation: tokio::sync::Mutex<()>,
}

impl Flush {
    fn begin(&self) -> UnboundedReceiver<Reply> {
        let (tx, rx) = unbounded_channel();
        *self.sender.lock().unwrap_or_else(PoisonError::into_inner) = Some(tx);
        rx
    }

    fn end(&self) {
        *self.sender.lock().unwrap_or_else(PoisonError::into_inner) = None;
    }

    /// Ответ окна `label`; `false` — сейчас никто не ждёт.
    pub fn reply(&self, label: &str, state: String) -> bool {
        self.sender
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .as_ref()
            .is_some_and(|sender| sender.send((label.to_string(), state)).is_ok())
    }
}

/// Ждёт ответы окон из `pending`. `true` — можно выходить: все ответили `clean`/`proceed` либо молчат за
/// `quick` и ни одно не задало вопрос; молчание после вопроса пользователю за `patient` — нельзя.
async fn wait(
    rx: &mut UnboundedReceiver<Reply>,
    mut pending: HashSet<String>,
    quick: Duration,
    patient: Duration,
    mut on_asking: impl FnMut(&str),
) -> bool {
    let mut asked: HashSet<String> = HashSet::new();
    while !pending.is_empty() {
        let limit = if asked.is_empty() { quick } else { patient };
        match tokio::time::timeout(limit, rx.recv()).await {
            Ok(Some((label, state))) => match state.as_str() {
                "clean" | "proceed" => {
                    pending.remove(&label);
                    asked.remove(&label);
                }
                "cancel" => return false,
                "asking" => {
                    asked.insert(label.clone());
                    on_asking(&label);
                }
                _ => {}
            },
            Ok(None) | Err(_) => return asked.is_empty(),
        }
    }
    true
}

/// Просит все окна записать несохранённые документы. `false` — пользователь или ошибка записи просят остаться.
pub async fn request(app: &AppHandle) -> bool {
    let state = app.state::<Shared>();
    let _operation = state.flush.operation.lock().await;
    let mut rx = state.flush.begin();
    if app.emit("bars://flush-request", ()).is_err() {
        state.flush.end();
        return true;
    }
    let pending: HashSet<String> = app.webview_windows().into_keys().collect();
    let shown = app.clone();
    let proceed = wait(&mut rx, pending, QUICK, PATIENT, move |label| {
        if let Some(window) = shown.get_webview_window(label) {
            let _ = window.show();
            let _ = window.unminimize();
            let _ = window.set_focus();
        }
    })
    .await;
    state.flush.end();
    proceed
}

/// Перед переименованием все окна подтверждают запись; молчание запрещает операцию.
pub async fn request_clean(app: &AppHandle) -> bool {
    let state = app.state::<Shared>();
    let _operation = state.flush.operation.lock().await;
    let mut rx = state.flush.begin();
    let mut pending: HashSet<String> = app.webview_windows().into_keys().collect();
    if app.emit("bars://move-flush-request", ()).is_err() {
        state.flush.end();
        return false;
    }
    let result = tokio::time::timeout(Duration::from_secs(10), async {
        while !pending.is_empty() {
            match rx.recv().await {
                Some((label, reply)) if reply == "clean" => {
                    pending.remove(&label);
                }
                _ => return false,
            }
        }
        true
    })
    .await
    .unwrap_or(false);
    state.flush.end();
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    const SHORT: Duration = Duration::from_millis(60);

    fn replies(states: &[(&str, &str)]) -> UnboundedReceiver<Reply> {
        let (tx, rx) = unbounded_channel();
        for (label, state) in states {
            tx.send(((*label).to_string(), (*state).to_string()))
                .unwrap();
        }
        // Отправитель живёт, пока жив тест: закрытый канал — отдельный случай.
        std::mem::forget(tx);
        rx
    }

    fn windows(labels: &[&str]) -> HashSet<String> {
        labels.iter().map(|label| (*label).to_string()).collect()
    }

    #[tokio::test]
    async fn clean_window_lets_the_process_exit() {
        let mut rx = replies(&[("main", "clean")]);
        assert!(wait(&mut rx, windows(&["main"]), SHORT, SHORT, |_| {}).await);
    }

    #[tokio::test]
    async fn silent_window_does_not_block_exit() {
        let mut rx = replies(&[]);
        assert!(wait(&mut rx, windows(&["main"]), SHORT, SHORT, |_| {}).await);
    }

    #[tokio::test]
    async fn user_can_discard_or_stay_after_the_window_asked() {
        let mut asked = Vec::new();
        let mut rx = replies(&[("main", "asking"), ("main", "proceed")]);
        assert!(
            wait(&mut rx, windows(&["main"]), SHORT, SHORT, |label| asked
                .push(label.to_string()))
            .await
        );
        assert_eq!(asked, ["main"]);
        let mut rx = replies(&[("main", "asking"), ("main", "cancel")]);
        assert!(!wait(&mut rx, windows(&["main"]), SHORT, SHORT, |_| {}).await);
    }

    #[tokio::test]
    async fn unanswered_question_keeps_the_process_running() {
        let mut rx = replies(&[("main", "asking")]);
        assert!(!wait(&mut rx, windows(&["main"]), SHORT, SHORT, |_| {}).await);
    }

    #[tokio::test]
    async fn unknown_replies_are_ignored() {
        let mut rx = replies(&[("main", "???"), ("main", "clean")]);
        assert!(wait(&mut rx, windows(&["main"]), SHORT, SHORT, |_| {}).await);
    }

    #[tokio::test]
    async fn every_window_is_asked_not_only_the_first_to_answer() {
        // Отдельное окно отвечает «спрашиваю» после того, как основное уже ответило «чисто».
        let mut asked = Vec::new();
        let mut rx = replies(&[("main", "clean"), ("doc-1", "asking")]);
        assert!(
            !wait(
                &mut rx,
                windows(&["main", "doc-1"]),
                SHORT,
                SHORT,
                |label| asked.push(label.to_string())
            )
            .await
        );
        assert_eq!(asked, ["doc-1"]);
        let mut rx = replies(&[("main", "clean"), ("doc-1", "asking"), ("doc-1", "proceed")]);
        assert!(wait(&mut rx, windows(&["main", "doc-1"]), SHORT, SHORT, |_| {}).await);
    }

    #[test]
    fn reply_reaches_only_the_active_request() {
        let flush = Flush::default();
        assert!(!flush.reply("main", "clean".into()));
        let mut rx = flush.begin();
        assert!(flush.reply("doc-1", "clean".into()));
        assert_eq!(
            rx.try_recv().unwrap(),
            ("doc-1".to_string(), "clean".to_string())
        );
        flush.end();
        assert!(!flush.reply("main", "clean".into()));
    }
}
