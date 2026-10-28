//! Внутренний API окна для ссылок документа (`docs/features/knowledge-base/link-index.md`, «Вызовы воркера»).
//!
//! Метод только для окна: он читает индекс ссылок `.bars/links.sqlite3`, а не граф, и работает без модели.
//! Проект — абсолютный путь корня, как у `new_topics_*`.

use serde::{Deserialize, Serialize};

/// Исходящие и входящие ссылки одного документа.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LinksOfParams {
    pub project: String,
    /// Путь документа от корня проекта, как в дереве и выдаче.
    pub path: String,
}

/// Исходящая ссылка: адресат разрешён по документам проекта на диске.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OutgoingLinkItem {
    pub line: u32,
    /// Видимый текст ссылки.
    pub text: String,
    /// Адресат как написан в документе.
    pub raw: String,
    /// `inline`, `reference`, `wiki`.
    pub kind: String,
    /// Путь документа-адресата от корня проекта; нет у битой ссылки.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_title: Option<String>,
    /// Адресата нет среди документов проекта.
    pub broken: bool,
}

/// Документ-источник входящей ссылки.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IncomingLinkItem {
    pub path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// Строки источника, где стоит ссылка.
    pub lines: Vec<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LinksOfOutput {
    /// Документ есть среди документов проекта на диске; иначе списки пусты.
    pub known: bool,
    pub outgoing: Vec<OutgoingLinkItem>,
    pub incoming: Vec<IncomingLinkItem>,
}

/// Входящие ссылки на набор документов из документов вне набора: что станет битым при удалении.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LinksIntoParams {
    pub project: String,
    /// Документы набора, пути от корня проекта (файл или все документы папки).
    pub paths: Vec<String>,
}

/// Ссылка на документ набора: строка источника и видимый текст.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LinkIntoItem {
    pub line: u32,
    pub text: String,
}

/// Документ вне набора и его ссылки на набор.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LinksIntoSource {
    pub path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    pub links: Vec<LinkIntoItem>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LinksIntoOutput {
    pub sources: Vec<LinksIntoSource>,
}
