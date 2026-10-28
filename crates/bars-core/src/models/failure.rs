//! Вид ошибки встроенной модели (`docs/features/window/error-states.md`, «Виды ошибок
//! встроенной модели»): окно выбирает экран по виду, а не по тексту.

use hf_hub::api::sync::ApiError;

/// Что случилось со встроенной моделью; значения `data.reason` у `provider_error`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModelErrorKind {
    /// Скачивание не дошло до huggingface.co: нет сети, таймаут, ответ сервера с ошибкой.
    Network,
    /// Нет нужных файлов, файл пуст, не сошёлся размер или хеш.
    FilesMissing,
    /// Не хватило оперативной или видеопамяти.
    OutOfMemory,
    /// Всё остальное: сбой ORT, DirectML, Vulkan, llama.cpp, нет места на диске.
    Runtime,
}

impl ModelErrorKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Network => "network",
            Self::FilesMissing => "files_missing",
            Self::OutOfMemory => "out_of_memory",
            Self::Runtime => "runtime",
        }
    }
}

/// Ошибка получения или загрузки модели: вид вместе с безопасным сообщением.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelFailure {
    pub kind: ModelErrorKind,
    pub message: String,
}

impl ModelFailure {
    pub fn new(kind: ModelErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
        }
    }

    /// Ошибка из текста библиотеки: только нехватка памяти или `runtime`.
    pub fn from_library(message: impl Into<String>) -> Self {
        let message = message.into();
        Self {
            kind: library_kind(&message),
            message,
        }
    }
}

impl std::fmt::Display for ModelFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

/// Признаки нехватки памяти в тексте ошибки библиотеки; один список и для вида ошибки, и для
/// уменьшения пачки rerank.
const OUT_OF_MEMORY_MARKERS: [&str; 7] = [
    "out of memory",
    "outofmemory",
    "e_outofmemory",
    "0x8007000e",
    "failed to allocate",
    "bad allocation",
    "not enough memory",
];

pub fn is_out_of_memory(message: &str) -> bool {
    let message = message.to_ascii_lowercase();
    OUT_OF_MEMORY_MARKERS
        .iter()
        .any(|marker| message.contains(marker))
}

/// Вид ошибки там, где её формирует библиотека: слова про файлы и сеть в её тексте вид не меняют,
/// «файлов нет» выставляется только проверкой файлов bars.
pub fn library_kind(message: &str) -> ModelErrorKind {
    if is_out_of_memory(message) {
        ModelErrorKind::OutOfMemory
    } else {
        ModelErrorKind::Runtime
    }
}

/// Вид ошибки скачивания через hf-hub: запись на диск — `runtime` (нет места), повреждённая часть
/// файла — `files_missing`, остальное (запрос, заголовки, повторы) — `network`.
pub(super) fn hub_kind(error: &ApiError) -> ModelErrorKind {
    match error {
        ApiError::IoError(_) => ModelErrorKind::Runtime,
        ApiError::InvalidResume => ModelErrorKind::FilesMissing,
        ApiError::TooManyRetries(inner) => hub_kind(inner),
        _ => ModelErrorKind::Network,
    }
}

/// Ошибка hf-hub как ошибка загрузки модели с видом по [`hub_kind`].
pub(super) fn hub_embed_error(error: ApiError) -> super::EmbedError {
    let message = error.to_string();
    match hub_kind(&error) {
        ModelErrorKind::Network => super::EmbedError::Network(message),
        ModelErrorKind::FilesMissing => super::EmbedError::Files(message),
        _ => super::EmbedError::Load(message),
    }
}

impl From<ModelFailure> for super::EmbedError {
    fn from(failure: ModelFailure) -> Self {
        match failure.kind {
            ModelErrorKind::Network => Self::Network(failure.message),
            ModelErrorKind::FilesMissing => Self::Files(failure.message),
            _ => Self::Load(failure.message),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn library_text_gives_only_out_of_memory_or_runtime() {
        for text in [
            "ORT: Out Of Memory",
            "DirectML: 0x8007000E",
            "failed to allocate 1 GiB",
            "E_OUTOFMEMORY",
            "bad allocation",
        ] {
            assert_eq!(library_kind(text), ModelErrorKind::OutOfMemory, "{text}");
        }
        for text in [
            "файл model.onnx не найден",
            "network unreachable huggingface.co",
            "сбой Vulkan",
            "",
        ] {
            assert_eq!(library_kind(text), ModelErrorKind::Runtime, "{text}");
        }
    }

    #[test]
    fn hub_errors_split_into_network_disk_and_corrupted_part() {
        let io = || std::io::Error::other("диск полон");
        assert_eq!(hub_kind(&ApiError::IoError(io())), ModelErrorKind::Runtime);
        assert_eq!(
            hub_kind(&ApiError::TooManyRetries(Box::new(ApiError::IoError(io())))),
            ModelErrorKind::Runtime
        );
        assert_eq!(
            hub_kind(&ApiError::InvalidResume),
            ModelErrorKind::FilesMissing
        );
        assert_eq!(
            hub_kind(&ApiError::TooManyRetries(Box::new(ApiError::InvalidResume))),
            ModelErrorKind::FilesMissing
        );
        assert_eq!(
            hub_kind(&ApiError::ParseIntError("x".parse::<u8>().unwrap_err())),
            ModelErrorKind::Network
        );
    }

    #[test]
    fn names_follow_the_protocol() {
        assert_eq!(ModelErrorKind::Network.as_str(), "network");
        assert_eq!(ModelErrorKind::FilesMissing.as_str(), "files_missing");
        assert_eq!(ModelErrorKind::OutOfMemory.as_str(), "out_of_memory");
        assert_eq!(ModelErrorKind::Runtime.as_str(), "runtime");
    }
}
