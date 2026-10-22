//! Чтение и запись документа для редактора окна (Т-29, НФТ-5): допуск пути, ревизия файла,
//! запись «сравнить и заменить» через временный файл. Без Tauri: команды в `commands.rs` только вызывают это.

use std::fs;
use std::io::{ErrorKind, Write};
use std::path::{Component, Path, PathBuf};

/// Больше этого размера документ открывается только для чтения.
pub const MAX_EDITABLE: u64 = 5 * 1024 * 1024;
/// Больше этого размера документ не читается в окно совсем.
pub const MAX_READABLE: u64 = 50 * 1024 * 1024;

#[derive(Debug, PartialEq, Eq)]
pub enum DocError {
    InvalidPath,
    OutsideRoot,
    NotFound,
    TooLarge,
    ParentMissing,
    ReadOnly,
    Locked,
    Io(String),
}

impl DocError {
    pub fn kind(&self) -> &'static str {
        match self {
            Self::InvalidPath => "invalid_path",
            Self::OutsideRoot => "outside_root",
            Self::NotFound => "not_found",
            Self::TooLarge => "too_large",
            Self::ParentMissing => "parent_missing",
            Self::ReadOnly => "read_only",
            Self::Locked => "locked",
            Self::Io(_) => "io",
        }
    }

    pub fn message(&self) -> String {
        match self {
            Self::InvalidPath => "Недопустимый путь документа".into(),
            Self::OutsideRoot => "Документ вне корня документации проекта".into(),
            Self::NotFound => "Файл не найден".into(),
            Self::TooLarge => "Файл слишком велик для окна".into(),
            Self::ParentMissing => "Папки документа нет".into(),
            Self::ReadOnly => "Файл только для чтения".into(),
            Self::Locked => "Файл занят другой программой".into(),
            Self::Io(text) => format!("Ошибка файловой системы: {text}"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Eol {
    Lf,
    Crlf,
}

impl Eol {
    pub fn name(self) -> &'static str {
        match self {
            Self::Lf => "lf",
            Self::Crlf => "crlf",
        }
    }

    pub fn parse(name: &str) -> Self {
        if name == "crlf" { Self::Crlf } else { Self::Lf }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReadOnlyReason {
    NotUtf8,
    TooLarge,
    File,
}

impl ReadOnlyReason {
    pub fn name(self) -> &'static str {
        match self {
            Self::NotUtf8 => "not_utf8",
            Self::TooLarge => "too_large",
            Self::File => "file",
        }
    }
}

#[derive(Debug)]
pub struct ReadOut {
    /// Перевод строк приведён к `\n`.
    pub text: String,
    pub revision: String,
    pub eol: Eol,
    pub bom: bool,
    pub size: u64,
    pub read_only: Option<ReadOnlyReason>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum WriteOutcome {
    Saved {
        revision: String,
    },
    /// Файл на диске не тот, что ожидало окно; `current` — его ревизия, `None` — файла нет.
    Conflict {
        current: Option<String>,
    },
}

/// Ревизия — размер и хеш байтов: mtime у OneDrive и копирования ненадёжен.
pub fn revision_of(bytes: &[u8]) -> String {
    let hash = blake3::hash(bytes).to_hex();
    format!("{}-{}", bytes.len(), &hash.as_str()[..32])
}

fn io_error(error: &std::io::Error) -> DocError {
    match error.kind() {
        ErrorKind::NotFound => DocError::NotFound,
        ErrorKind::PermissionDenied => DocError::Locked,
        _ => match error.raw_os_error() {
            // ERROR_SHARING_VIOLATION и ERROR_LOCK_VIOLATION
            Some(32 | 33) => DocError::Locked,
            _ => DocError::Io(error.to_string()),
        },
    }
}

/// Абсолютный путь документа: `path` от корня проекта, внутри корня документации, `.md`,
/// без выхода через junction или symlink. Самого файла может не быть, папка должна существовать.
pub fn resolve(project: &Path, docs_root: &str, path: &str) -> Result<PathBuf, DocError> {
    let relative =
        crate::documents::indexed_path(Path::new(""), path).map_err(|_| DocError::InvalidPath)?;
    let docs: PathBuf = Path::new(&docs_root.replace('\\', "/"))
        .components()
        .filter(|part| !matches!(part, Component::CurDir))
        .collect();
    if docs
        .components()
        .any(|part| !matches!(part, Component::Normal(_)))
    {
        return Err(DocError::OutsideRoot);
    }
    if !relative.starts_with(&docs) {
        return Err(DocError::OutsideRoot);
    }
    let project = project.canonicalize().map_err(|_| DocError::OutsideRoot)?;
    let root = project
        .join(&docs)
        .canonicalize()
        .map_err(|_| DocError::OutsideRoot)?;
    if !root.starts_with(&project) || !root.is_dir() {
        return Err(DocError::OutsideRoot);
    }
    let target = project.join(&relative);
    match fs::symlink_metadata(&target) {
        Ok(_) => {
            let real = target.canonicalize().map_err(|_| DocError::OutsideRoot)?;
            if !real.starts_with(&root) {
                return Err(DocError::OutsideRoot);
            }
            if !real.is_file() {
                return Err(DocError::InvalidPath);
            }
            Ok(real)
        }
        Err(error) if error.kind() == ErrorKind::NotFound => {
            let parent = target.parent().ok_or(DocError::InvalidPath)?;
            let real_parent = parent.canonicalize().map_err(|_| DocError::ParentMissing)?;
            if !real_parent.starts_with(&root) {
                return Err(DocError::OutsideRoot);
            }
            if !real_parent.is_dir() {
                return Err(DocError::ParentMissing);
            }
            Ok(real_parent.join(target.file_name().ok_or(DocError::InvalidPath)?))
        }
        Err(error) => Err(io_error(&error)),
    }
}

fn detect_eol(text: &str) -> Eol {
    let crlf = text.matches("\r\n").count();
    let lf = text.matches('\n').count() - crlf;
    if crlf > lf { Eol::Crlf } else { Eol::Lf }
}

pub fn read(file: &Path) -> Result<ReadOut, DocError> {
    let metadata = fs::metadata(file).map_err(|e| io_error(&e))?;
    if !metadata.is_file() {
        return Err(DocError::InvalidPath);
    }
    if metadata.len() > MAX_READABLE {
        return Err(DocError::TooLarge);
    }
    let bytes = fs::read(file).map_err(|e| io_error(&e))?;
    let revision = revision_of(&bytes);
    let bom = bytes.starts_with(&[0xEF, 0xBB, 0xBF]);
    let body = if bom { &bytes[3..] } else { &bytes[..] };
    let (raw, mut read_only) = match std::str::from_utf8(body) {
        Ok(text) => (text.to_owned(), None),
        Err(_) => (
            String::from_utf8_lossy(body).into_owned(),
            Some(ReadOnlyReason::NotUtf8),
        ),
    };
    if read_only.is_none() && bytes.len() as u64 > MAX_EDITABLE {
        read_only = Some(ReadOnlyReason::TooLarge);
    }
    if read_only.is_none() && metadata.permissions().readonly() {
        read_only = Some(ReadOnlyReason::File);
    }
    let eol = detect_eol(&raw);
    Ok(ReadOut {
        text: raw.replace("\r\n", "\n"),
        revision,
        eol,
        bom,
        size: bytes.len() as u64,
        read_only,
    })
}

/// Ревизия файла на диске; `None` — файла нет.
pub fn revision(file: &Path) -> Result<Option<String>, DocError> {
    match fs::read(file) {
        Ok(bytes) => Ok(Some(revision_of(&bytes))),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(None),
        Err(error) => Err(io_error(&error)),
    }
}

fn encode(text: &str, eol: Eol, bom: bool) -> Vec<u8> {
    let body = match eol {
        Eol::Lf => text.to_owned(),
        Eol::Crlf => text.replace('\n', "\r\n"),
    };
    let mut bytes = Vec::with_capacity(body.len() + 3);
    if bom {
        bytes.extend_from_slice(&[0xEF, 0xBB, 0xBF]);
    }
    bytes.extend_from_slice(body.as_bytes());
    bytes
}

/// Записывает `text`, только если файл на диске ещё в ревизии `base` (`None` — файла быть не должно).
/// Файл меняется атомарно: временный файл рядом и переименование поверх оригинала.
pub fn write(
    file: &Path,
    text: &str,
    base: Option<&str>,
    eol: Eol,
    bom: bool,
) -> Result<WriteOutcome, DocError> {
    let current = revision(file)?;
    if current.as_deref() != base {
        return Ok(WriteOutcome::Conflict { current });
    }
    if current.is_some()
        && fs::metadata(file)
            .map_err(|e| io_error(&e))?
            .permissions()
            .readonly()
    {
        return Err(DocError::ReadOnly);
    }
    let parent = file.parent().ok_or(DocError::InvalidPath)?;
    if !parent.is_dir() {
        return Err(DocError::ParentMissing);
    }
    let bytes = encode(text, eol, bom);
    let name = file.file_name().ok_or(DocError::InvalidPath)?;
    let mut temp_name = std::ffi::OsString::from(".");
    temp_name.push(name);
    temp_name.push(".bars-tmp");
    let temp = parent.join(temp_name);
    let result = (|| -> Result<WriteOutcome, DocError> {
        let mut handle = fs::File::create(&temp).map_err(|e| io_error(&e))?;
        handle.write_all(&bytes).map_err(|e| io_error(&e))?;
        handle.sync_all().map_err(|e| io_error(&e))?;
        drop(handle);
        let revision = revision_of(&bytes);
        if base.is_none() {
            // Новый файл: жёсткая ссылка не заменяет чужой файл, появившийся после проверки.
            match fs::hard_link(&temp, file) {
                Ok(()) => return Ok(WriteOutcome::Saved { revision }),
                Err(error) if error.kind() == ErrorKind::AlreadyExists => {
                    return Ok(WriteOutcome::Conflict {
                        current: self::revision(file)?,
                    });
                }
                Err(_) => {}
            }
        }
        fs::rename(&temp, file).map_err(|e| io_error(&e))?;
        Ok(WriteOutcome::Saved { revision })
    })();
    // После успешного переименования временного файла нет; после ссылки или ошибки — убираем.
    let _ = fs::remove_file(&temp);
    result
}
