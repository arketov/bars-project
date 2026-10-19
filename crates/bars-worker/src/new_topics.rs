//! Сбор «Новых тем» в воркере (`docs/features/new-topics/design.md`).
//!
//! Один писатель — поток этого модуля. Поиск не ждёт записи: наблюдение уходит в ограниченную очередь
//! (`try_send`), переполнение и ошибки БД считаются потерями без текстов запросов в логе.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, SyncSender, sync_channel};
use std::sync::{Arc, Mutex};

use bars_core::new_topics::{
    Observation, Recorded, StoreError, TopicRow, TopicStore, store_path,
};
use serde_json::Value;

/// Очередь ожидающих записи наблюдений на весь воркер.
const QUEUE: usize = 256;

/// Откуда пришёл поисковый вызов.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Origin {
    #[default]
    Window,
    Mcp,
    Cli,
}

/// Назначение поиска: считается ли он самостоятельным обращением (NT-03).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Purpose {
    /// Обращение пользователя или агента.
    #[default]
    Request,
    /// Диагностический запрос окна («почему пусто»), повторная проверка темы: спрос не создаёт.
    Diagnostic,
}

/// Внутренний контекст поискового вызова; публичных аргументов MCP не добавляет.
#[derive(Debug, Clone, Default)]
pub struct SearchContext {
    pub origin: Origin,
    pub purpose: Purpose,
    pub request_id: Option<String>,
    pub parent_request_id: Option<String>,
}

impl SearchContext {
    /// Вынимает служебное поле `_window` из параметров вызова окна. Поле — не часть протокола:
    /// из параметров оно удаляется до разбора вызова.
    pub fn take_from(params: &mut Value) -> Self {
        let mut context = Self::default();
        let Some(Value::Object(map)) = params.as_object_mut().and_then(|m| m.remove("_window"))
        else {
            return context;
        };
        if map.get("purpose").and_then(Value::as_str) == Some("diagnostic") {
            context.purpose = Purpose::Diagnostic;
        }
        context.request_id = map
            .get("request_id")
            .and_then(Value::as_str)
            .filter(|id| !id.is_empty() && id.len() <= 128)
            .map(str::to_owned);
        context.parent_request_id = map
            .get("parent_request_id")
            .and_then(Value::as_str)
            .filter(|id| !id.is_empty() && id.len() <= 128)
            .map(str::to_owned);
        context
    }
}

tokio::task_local! {
    pub static CONTEXT: SearchContext;
}

/// Контекст текущего вызова; без областей — вызов окна без служебных полей.
pub fn current_context() -> SearchContext {
    CONTEXT.try_with(Clone::clone).unwrap_or_default()
}

struct Message {
    project: PathBuf,
    observation: Observation,
}

struct Inner {
    sender: SyncSender<Message>,
    stores: Mutex<HashMap<PathBuf, TopicStore>>,
    /// Поколение и включённость по проектам для допуска без обращения к БД.
    admissions: Mutex<HashMap<PathBuf, (i64, bool)>>,
    /// Проекты, которые удаляются: записи и открытие БД для них запрещены.
    blocked: Mutex<HashSet<PathBuf>>,
    lost: AtomicU64,
    last_error: Mutex<Option<String>>,
    next_id: AtomicU64,
    epoch: String,
}

/// Сбор тем всех проектов воркера.
#[derive(Clone)]
pub struct Collector(Arc<Inner>);

fn with_store<T>(
    inner: &Inner,
    project: &Path,
    action: impl FnOnce(&mut TopicStore) -> Result<T, StoreError>,
) -> Result<T, StoreError> {
    let mut stores = inner.stores.lock().unwrap_or_else(|e| e.into_inner());
    let io = |kind, text: &str| StoreError::Io(std::io::Error::new(kind, text.to_string()));
    if inner
        .blocked
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .contains(project)
    {
        return Err(io(std::io::ErrorKind::PermissionDenied, "проект удаляется"));
    }
    if !stores.contains_key(project) {
        // Папку `.bars` создаёт проект; запись не воскрешает её после удаления проекта.
        if !store_path(project).parent().is_some_and(Path::is_dir) {
            return Err(io(std::io::ErrorKind::NotFound, "нет папки .bars проекта"));
        }
        stores.insert(project.to_path_buf(), TopicStore::open(project)?);
    }
    let store = stores.get_mut(project).expect("только что открыт");
    action(store)
}

/// Тип ошибки без текстов запросов: пути и сообщения SQLite не содержат вопросов, но коротко.
fn describe(error: &StoreError) -> String {
    match error {
        StoreError::LimitReached(limit) => format!("достигнут предел {limit} тем"),
        StoreError::NewerSchema(version) => format!("схема БД новее поддерживаемой ({version})"),
        StoreError::Sqlite(_) => "ошибка БД новых тем".to_string(),
        StoreError::Io(_) => "не создана папка БД новых тем".to_string(),
    }
}

impl Inner {
    fn lose(&self, reason: String) {
        self.lost.fetch_add(1, Ordering::Relaxed);
        *self.last_error.lock().unwrap_or_else(|e| e.into_inner()) = Some(reason);
    }

    fn write(&self, message: Message) {
        match with_store(self, &message.project, |store| {
            store.record(&message.observation)
        }) {
            Ok(Recorded::Created { .. } | Recorded::Merged { .. }) => {
                log::debug!("Новые темы: наблюдение записано");
            }
            Ok(other) => log::debug!("Новые темы: наблюдение не добавлено ({other:?})"),
            Err(error) => {
                let reason = describe(&error);
                log::warn!("Новые темы: потеряно наблюдение: {reason}");
                self.lose(reason);
            }
        }
    }
}

fn run(inner: std::sync::Weak<Inner>, receiver: Receiver<Message>) {
    loop {
        let message = match receiver.recv_timeout(std::time::Duration::from_secs(1)) {
            Ok(message) => message,
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                if inner.strong_count() == 0 {
                    return;
                }
                continue;
            }
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => return,
        };
        let Some(inner) = inner.upgrade() else {
            return;
        };
        inner.write(message);
    }
}

impl Default for Collector {
    fn default() -> Self {
        Self::new()
    }
}

impl Collector {
    pub fn new() -> Self {
        let (sender, receiver) = sync_channel(QUEUE);
        let inner = Arc::new(Inner {
            sender,
            stores: Mutex::new(HashMap::new()),
            admissions: Mutex::new(HashMap::new()),
            blocked: Mutex::new(HashSet::new()),
            lost: AtomicU64::new(0),
            last_error: Mutex::new(None),
            next_id: AtomicU64::new(0),
            epoch: format!(
                "{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_nanos()
            ),
        });
        let weak = Arc::downgrade(&inner);
        std::thread::Builder::new()
            .name("new-topics-writer".into())
            .spawn(move || run(weak, receiver))
            .expect("поток записи новых тем");
        Self(inner)
    }

    /// ID допущенного обращения: уникален в пределах запуска воркера и его epoch.
    pub fn next_request_id(&self, origin: Origin) -> String {
        let n = self.0.next_id.fetch_add(1, Ordering::Relaxed);
        let kind = match origin {
            Origin::Window => "window",
            Origin::Mcp => "mcp",
            Origin::Cli => "cli",
        };
        format!("{kind}:{}:{n}", self.0.epoch)
    }

    /// Поколение списка проекта, если сбор включён и БД доступна; иначе `None` (наблюдение не создаётся).
    /// Первое обращение к проекту читает БД не на пути поиска (`spawn_blocking`).
    pub async fn admit(&self, project: &Path) -> Option<i64> {
        if let Some((generation, enabled)) = self
            .0
            .admissions
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(project)
            .copied()
        {
            return enabled.then_some(generation);
        }
        let inner = self.0.clone();
        let path = project.to_path_buf();
        let loaded = tokio::task::spawn_blocking(move || {
            with_store(&inner, &path, |store| store.settings()).map(|value| (path, value))
        })
        .await;
        match loaded {
            Ok(Ok((path, value))) => {
                self.0
                    .admissions
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .insert(path, value);
                value.1.then_some(value.0)
            }
            Ok(Err(error)) => {
                let reason = describe(&error);
                log::warn!("Новые темы недоступны: {reason}");
                self.0.lose(reason);
                None
            }
            Err(_) => None,
        }
    }

    /// Ставит наблюдение в очередь записи; поиск не ждёт. Переполнение — учтённая потеря.
    pub fn submit(&self, project: PathBuf, observation: Observation) {
        match self.0.sender.try_send(Message {
            project,
            observation,
        }) {
            Ok(()) => {}
            Err(_) => {
                log::warn!("Новые темы: очередь записи переполнена, наблюдение потеряно");
                self.0.lose("очередь записи переполнена".to_string());
            }
        }
    }

    /// Число известных потерь с запуска воркера и причина последней.
    pub fn losses(&self) -> (u64, Option<String>) {
        (
            self.0.lost.load(Ordering::Relaxed),
            self.0
                .last_error
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .clone(),
        )
    }

    async fn blocking<T: Send + 'static>(
        &self,
        project: &Path,
        action: impl FnOnce(&mut TopicStore) -> Result<T, StoreError> + Send + 'static,
    ) -> Result<T, StoreError> {
        let inner = self.0.clone();
        let path = project.to_path_buf();
        tokio::task::spawn_blocking(move || with_store(&inner, &path, action))
            .await
            .map_err(|_| StoreError::Io(std::io::Error::other("задача БД новых тем прервана")))?
    }

    fn remember(&self, project: &Path, generation: i64, enabled: bool) {
        self.0
            .admissions
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(project.to_path_buf(), (generation, enabled));
    }

    /// Сбор вкл/выкл. Смена поколения отклоняет ещё не записанные наблюдения.
    pub async fn set_enabled(&self, project: &Path, enabled: bool) -> Result<i64, StoreError> {
        let generation = self
            .blocking(project, move |store| store.set_enabled(enabled))
            .await?;
        self.remember(project, generation, enabled);
        Ok(generation)
    }

    /// Очистка списка; сбор остаётся включённым или выключенным как был.
    pub async fn clear(&self, project: &Path) -> Result<i64, StoreError> {
        let (generation, enabled) = self
            .blocking(project, |store| {
                let generation = store.clear()?;
                Ok((generation, store.settings()?.1))
            })
            .await?;
        self.remember(project, generation, enabled);
        Ok(generation)
    }

    pub async fn delete_topic(&self, project: &Path, id: i64) -> Result<bool, StoreError> {
        self.blocking(project, move |store| store.delete_topic(id))
            .await
    }

    pub async fn list(&self, project: &Path) -> Result<Vec<TopicRow>, StoreError> {
        self.blocking(project, |store| store.list()).await
    }

    /// Перед удалением проекта: закрывает соединение и запрещает новые записи, чтобы БД не осталась
    /// открытой и не воскресла после удаления файлов. Завершается `end_delete`.
    pub fn begin_delete(&self, project: &Path) {
        // Порядок как в `with_store`: сначала stores, потом blocked.
        let mut stores = self.0.stores.lock().unwrap_or_else(|e| e.into_inner());
        self.0
            .blocked
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(project.to_path_buf());
        stores.remove(project);
        self.0
            .admissions
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(project);
    }

    /// Удаление проекта закончилось (успешно или нет): запись разрешена снова.
    pub fn end_delete(&self, project: &Path) {
        self.0
            .blocked
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(project);
    }
}
