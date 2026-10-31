//! Сброс правок окна перед выходом (НФТ-5): процесс просит окно записать документы и ждёт ответа.
//!
//! Окно отвечает командой `flush_reply`: `clean` — писать нечего или всё записано; `asking` — остались
//! документы, которые не записались (конфликт, ошибка), окно спрашивает пользователя; `proceed` — пользователь
//! согласился закрыть без записи; `cancel` — остаться. Окно с изменённым или записывающимся документом сначала
//! отвечает `writing`: пока оно пишет, процесс ждёт окончания записи до `WRITING` (OQ-174, ISS-93).

use std::collections::HashSet;
use std::sync::{Mutex, PoisonError};
use std::time::Duration;

use tauri::{AppHandle, Emitter, Manager};
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};

use crate::commands::Shared;

/// Сколько ждать первого ответа окна: окна может не быть или оно ещё не загрузилось.
const QUICK: Duration = Duration::from_secs(3);
/// Сколько ждать окончания записи после ответа окна «writing».
const WRITING: Duration = Duration::from_secs(60);
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

/// Чем кончилось ожидание окон.
#[derive(Debug, PartialEq, Eq)]
enum Outcome {
    /// Можно выходить.
    Proceed,
    /// Пользователь отказался или не ответил на вопрос.
    Stay,
    /// Окно ответило «writing», но запись не кончилась за отведённое время.
    StillWriting,
}

/// Сроки ожидания ответов окон.
#[derive(Clone, Copy)]
struct Limits {
    /// Первого слова окна: окна может не быть или оно ещё не загрузилось.
    quick: Duration,
    /// Окончания записи после «writing».
    writing: Duration,
    /// Решения пользователя после вопроса.
    patient: Duration,
}

const LIMITS: Limits = Limits {
    quick: QUICK,
    writing: WRITING,
    patient: PATIENT,
};

/// Ждёт ответы окон из `pending`. `Proceed` — все ответили `clean`/`proceed` либо молчат за `quick` и ни одно не
/// задало вопрос и не пишет; молчание после вопроса за `patient` — `Stay`, после «writing» за `writing` — `StillWriting`.
async fn wait(
    rx: &mut UnboundedReceiver<Reply>,
    mut pending: HashSet<String>,
    limits: Limits,
    mut on_asking: impl FnMut(&str),
) -> Outcome {
    let mut asked: HashSet<String> = HashSet::new();
    let mut writing: HashSet<String> = HashSet::new();
    while !pending.is_empty() {
        let limit = if !asked.is_empty() {
            limits.patient
        } else if !writing.is_empty() {
            limits.writing
        } else {
            limits.quick
        };
        match tokio::time::timeout(limit, rx.recv()).await {
            Ok(Some((label, state))) => match state.as_str() {
                "clean" | "proceed" => {
                    pending.remove(&label);
                    asked.remove(&label);
                    writing.remove(&label);
                }
                "cancel" => return Outcome::Stay,
                "writing" => {
                    writing.insert(label);
                }
                "asking" => {
                    writing.remove(&label);
                    asked.insert(label.clone());
                    on_asking(&label);
                }
                _ => {}
            },
            Ok(None) | Err(_) => {
                return if !asked.is_empty() {
                    Outcome::Stay
                } else if !writing.is_empty() {
                    Outcome::StillWriting
                } else {
                    Outcome::Proceed
                };
            }
        }
    }
    Outcome::Proceed
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
    let outcome = wait(&mut rx, pending, LIMITS, move |label| {
        if let Some(window) = shown.get_webview_window(label) {
            let _ = window.show();
            let _ = window.unminimize();
            let _ = window.set_focus();
        }
    })
    .await;
    state.flush.end();
    if outcome == Outcome::StillWriting {
        // Выход отменён: окно сообщает пользователю, что документ ещё записывается.
        let _ = app.emit("bars://flush-still-writing", ());
    }
    outcome == Outcome::Proceed
}

/// Перед переименованием все окна подтверждают запись; молчание запрещает операцию.
pub async fn request_clean(app: &AppHandle, project: &str) -> bool {
    let state = app.state::<Shared>();
    let _operation = state.flush.operation.lock().await;
    let mut rx = state.flush.begin();
    let mut pending: HashSet<String> = app.webview_windows().into_keys().collect();
    if app.emit("bars://move-flush-request", project).is_err() {
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
    const TEST: Limits = Limits {
        quick: SHORT,
        writing: Duration::from_millis(400),
        patient: SHORT,
    };

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

    async fn outcome(states: &[(&str, &str)], labels: &[&str]) -> Outcome {
        wait(&mut replies(states), windows(labels), TEST, |_| {}).await
    }

    #[tokio::test]
    async fn clean_window_lets_the_process_exit() {
        assert_eq!(
            outcome(&[("main", "clean")], &["main"]).await,
            Outcome::Proceed
        );
    }

    #[tokio::test]
    async fn silent_window_does_not_block_exit() {
        assert_eq!(outcome(&[], &["main"]).await, Outcome::Proceed);
    }

    #[tokio::test]
    async fn user_can_discard_or_stay_after_the_window_asked() {
        let mut asked = Vec::new();
        let mut rx = replies(&[("main", "asking"), ("main", "proceed")]);
        let result = wait(&mut rx, windows(&["main"]), TEST, |label| {
            asked.push(label.to_string());
        })
        .await;
        assert_eq!(result, Outcome::Proceed);
        assert_eq!(asked, ["main"]);
        assert_eq!(
            outcome(&[("main", "asking"), ("main", "cancel")], &["main"]).await,
            Outcome::Stay
        );
    }

    #[tokio::test]
    async fn unanswered_question_keeps_the_process_running() {
        assert_eq!(
            outcome(&[("main", "asking")], &["main"]).await,
            Outcome::Stay
        );
    }

    #[tokio::test]
    async fn unknown_replies_are_ignored() {
        assert_eq!(
            outcome(&[("main", "???"), ("main", "clean")], &["main"]).await,
            Outcome::Proceed
        );
    }

    #[tokio::test]
    async fn every_window_is_asked_not_only_the_first_to_answer() {
        // Отдельное окно отвечает «спрашиваю» после того, как основное уже ответило «чисто».
        let mut asked = Vec::new();
        let mut rx = replies(&[("main", "clean"), ("doc-1", "asking")]);
        let result = wait(&mut rx, windows(&["main", "doc-1"]), TEST, |label| {
            asked.push(label.to_string());
        })
        .await;
        assert_eq!(result, Outcome::Stay);
        assert_eq!(asked, ["doc-1"]);
        assert_eq!(
            outcome(
                &[("main", "clean"), ("doc-1", "asking"), ("doc-1", "proceed")],
                &["main", "doc-1"]
            )
            .await,
            Outcome::Proceed
        );
    }

    #[tokio::test]
    async fn exit_waits_for_a_window_that_is_writing() {
        // Запись дольше первых слов окна (QUICK): процесс не уходит, пока окно не сказало «clean».
        let (tx, mut rx) = unbounded_channel();
        tx.send(("main".to_string(), "writing".to_string()))
            .unwrap();
        let late = tokio::spawn(async move {
            tokio::time::sleep(SHORT * 3).await;
            tx.send(("main".to_string(), "clean".to_string())).unwrap();
        });
        let result = wait(&mut rx, windows(&["main"]), TEST, |_| {}).await;
        late.await.unwrap();
        assert_eq!(result, Outcome::Proceed);
    }

    #[tokio::test]
    async fn writing_that_never_ends_cancels_the_exit() {
        assert_eq!(
            outcome(&[("main", "writing")], &["main"]).await,
            Outcome::StillWriting
        );
    }

    #[tokio::test]
    async fn writing_window_that_fails_asks_the_user() {
        assert_eq!(
            outcome(
                &[("main", "writing"), ("main", "asking"), ("main", "proceed")],
                &["main"]
            )
            .await,
            Outcome::Proceed
        );
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
