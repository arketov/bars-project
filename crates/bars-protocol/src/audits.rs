//! Внутренний API окна для аудитов знаний (`docs/features/knowledge-base/audits.md`, Т-36).
//!
//! Чтение, запуск и отмена доступны MCP; расписание и игнорирование остаются в окне.
//! Аудит ничего не пишет в Markdown: он читает индекс ссылок и индекс чанков,
//! результат хранится в `.bars/audits.sqlite3`. Проект — абсолютный путь корня, как у `project_delete`.

use serde::{Deserialize, Serialize};

/// Состояния видов на экране: все виды проекта с последним результатом.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuditListParams {
    pub project: String,
}

/// Строка вида на экране аудитов.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AuditSummary {
    /// `broken_links`, `duplicates` или `contradictions`.
    pub kind: String,
    /// `never`, `queued`, `running`, `complete`, `partial`, `failed`.
    pub state: String,
    /// Идущий запуск начат (секунды Unix).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub running_since: Option<i64>,
    /// Конец последнего запуска любого итога.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_run_at: Option<i64>,
    /// Итог последнего запуска: `complete`, `partial`, `failed`, `cancelled`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_status: Option<String>,
    /// Причина неполного или упавшего запуска, без ключей и текстов документов.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// Полный запуск, чьи находки показываются; нет, пока полного запуска не было.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result_run_id: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result_at: Option<i64>,
    /// Находок в умолчаниях экрана (без исчезнувших).
    pub total: i64,
    /// Из них новых относительно прежнего полного запуска.
    pub new_count: i64,
    /// Исчезнувших относительно прежнего полного запуска.
    pub gone_count: i64,
    /// Находок под переключателем «Перекрытие содержания» (у `duplicates`).
    pub overlap_total: i64,
    /// `off`, `daily`, `weekly`, `on_change`.
    pub schedule: String,
    pub schedules: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AuditListOutput {
    pub audits: Vec<AuditSummary>,
    /// Индекс требует переиндексации: виды по текстам/векторам индекса не запускаются.
    pub reindex_required: bool,
    /// Идёт индексация: результат видов по индексу может быть неполным.
    pub indexing: bool,
}

/// Запуск вида. Идущий уже вид возвращает его же, чужой вид проекта ждёт очереди.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuditRunParams {
    pub project: String,
    pub kind: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuditRunOutput {
    /// Запуск принят: идёт или ждёт.
    pub accepted: bool,
    /// Этот вид уже шёл или ждал: новый запуск не создан.
    pub already: bool,
    /// Запуск стоит в очереди за другим аудитом проекта.
    pub queued: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuditCancelParams {
    pub project: String,
    pub kind: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuditCancelOutput {
    /// Был ли что отменять: идущий или ожидающий запуск.
    pub cancelled: bool,
}

/// Страница находок последнего полного запуска вида.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuditFindingsParams {
    pub project: String,
    pub kind: String,
    /// `main` или `overlap`; без значения — все.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group: Option<String>,
    /// Показывать исчезнувшие (серым, после остальных).
    #[serde(default)]
    pub include_gone: bool,
    #[serde(default)]
    pub include_ignored: bool,
    /// Смещение: `next` прошлой страницы.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cursor: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
}

/// Сводка находок вида: числа по файлам, причинам и видам; узнать масштаб проблемы без
/// постраничного чтения сотен находок (агентский `bars_audit action=summary`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuditSummaryParams {
    pub project: String,
    pub kind: String,
    /// Сужение по путям предметов находок (префиксы папок или файлы).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub paths: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuditFileCount {
    /// Путь документа от корня проекта.
    pub path: String,
    pub count: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuditReasonCount {
    pub reason: String,
    pub count: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuditTagCount {
    pub tag: String,
    pub count: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuditSummaryOutput {
    /// Полный запуск, из которого взяты числа; нет, пока такого не было.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run_id: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finished: Option<i64>,
    /// Находок под выбранный фильтр.
    pub total: i64,
    /// Файлы с числом находок, самые проблемные первыми.
    pub files: Vec<AuditFileCount>,
    /// Причины находок, самые частые первыми.
    pub reasons: Vec<AuditReasonCount>,
    /// Виды находок (broken, exact, near…), самые частые первыми.
    pub tags: Vec<AuditTagCount>,
}

/// Документ и место в нём, к которым относится находка.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuditSubject {
    /// Путь от корня проекта.
    pub path: String,
    pub first_line: u32,
    pub last_line: u32,
    pub heading: String,
    /// `index`, `session_log`, `manual`, `feature`, `architecture`, `decision`, `research`,
    /// `backlog`, `document`.
    pub role: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AuditFinding {
    pub fingerprint: String,
    pub ignored: bool,
    /// `new`, `known`, `gone`.
    pub standing: String,
    /// `broken`, `exact`, `near`, `contained`, `partial`, `semantic`, `num`, `slot`, `swap`.
    pub tag: String,
    /// `main` или `overlap`.
    pub group: String,
    pub reason: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub score: Option<f32>,
    pub subjects: Vec<AuditSubject>,
    /// Общий отрывок или текст ссылки.
    pub detail: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AuditFindingsOutput {
    /// Полный запуск, из которого взяты находки; нет, пока такого не было.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run_id: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finished: Option<i64>,
    pub items: Vec<AuditFinding>,
    /// Всего под выбранный фильтр.
    pub total: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next: Option<i64>,
}

/// Расписание вида (`off`, `daily`, `weekly`, `on_change` — последнее только у `broken_links`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuditScheduleParams {
    pub project: String,
    pub kind: String,
    pub mode: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuditScheduleOutput {
    pub mode: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuditIgnoreParams {
    pub project: String,
    pub kind: String,
    pub fingerprint: String,
    pub ignored: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuditIgnoreOutput {
    pub ignored: bool,
}
