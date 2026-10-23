//! Дерево документации по диску и операции с файлами и папками (Т-32, Т-33): допуск путей и имён,
//! создание, переименование и перемещение, корзина. Без Tauri: команды в `commands.rs` только вызывают это.
//! Устройство и сценарии — `docs/features/knowledge-base/tree-and-files.md`.

use std::fs;
use std::io::ErrorKind;
use std::path::{Component, Path, PathBuf};
use std::time::UNIX_EPOCH;

use crate::document_io::{self, Eol, WriteOutcome};

/// Служебный файл bars в корне документации (Т-8): из окна не создаётся и не меняется.
const INSTRUCTION_FILE: &str = "BARS_INSTRUCTION.md";
const MAX_NAME: usize = 200;

#[derive(Debug, PartialEq, Eq)]
pub enum TreeError {
    DocsRootUnavailable,
    InvalidPath,
    InvalidName(String),
    InvalidMove(String),
    /// Узел с таким именем (без учёта регистра) уже есть; внутри — его имя на диске.
    Exists(String),
    OutsideRoot,
    NotFound,
    ParentMissing,
    Locked,
    TrashUnavailable(String),
    Io(String),
}

impl TreeError {
    pub fn kind(&self) -> &'static str {
        match self {
            Self::DocsRootUnavailable => "docs_root_unavailable",
            Self::InvalidPath => "invalid_path",
            Self::InvalidName(_) => "invalid_name",
            Self::InvalidMove(_) => "invalid_move",
            Self::Exists(_) => "exists",
            Self::OutsideRoot => "outside_root",
            Self::NotFound => "not_found",
            Self::ParentMissing => "parent_missing",
            Self::Locked => "locked",
            Self::TrashUnavailable(_) => "trash_unavailable",
            Self::Io(_) => "io",
        }
    }

    pub fn message(&self) -> String {
        match self {
            Self::DocsRootUnavailable => "Корень документации недоступен".into(),
            Self::InvalidPath => "Недопустимый путь".into(),
            Self::InvalidName(why) => format!("Недопустимое имя: {why}"),
            Self::InvalidMove(why) => format!("Так переместить нельзя: {why}"),
            Self::Exists(name) => format!("«{name}» уже есть в этой папке"),
            Self::OutsideRoot => "Путь вне корня документации проекта".into(),
            Self::NotFound => "Файла или папки нет".into(),
            Self::ParentMissing => "Папки нет".into(),
            Self::Locked => "Файл или папка заняты другой программой".into(),
            Self::TrashUnavailable(why) => {
                format!("Корзина недоступна, ничего не удалено: {why}")
            }
            Self::Io(text) => format!("Ошибка файловой системы: {text}"),
        }
    }
}

fn io_error(error: &std::io::Error) -> TreeError {
    match error.kind() {
        ErrorKind::NotFound => TreeError::NotFound,
        ErrorKind::PermissionDenied => TreeError::Locked,
        _ => match error.raw_os_error() {
            // ERROR_SHARING_VIOLATION и ERROR_LOCK_VIOLATION
            Some(32 | 33) => TreeError::Locked,
            _ => TreeError::Io(error.to_string()),
        },
    }
}

fn doc_error(error: document_io::DocError) -> TreeError {
    use document_io::DocError as D;
    match error {
        D::InvalidPath => TreeError::InvalidPath,
        D::OutsideRoot => TreeError::OutsideRoot,
        D::NotFound => TreeError::NotFound,
        D::ParentMissing => TreeError::ParentMissing,
        D::Locked => TreeError::Locked,
        other => TreeError::Io(other.message()),
    }
}

/// Файл дерева: путь от корня проекта через `/`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TreeFile {
    pub path: String,
    pub size: u64,
    /// Миллисекунды от эпохи; 0, если файловая система времени не отдаёт.
    pub modified_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TreeOut {
    pub docs_root: String,
    pub files: Vec<TreeFile>,
    pub dirs: Vec<String>,
    pub problems: Vec<(String, String)>,
    /// Хеш списка путей, размеров и времён: окно не перерисовывает неизменившееся дерево.
    pub revision: String,
}

/// Дерево с диска: те же файлы, что видит индексатор, и папки (в том числе пустые).
pub fn read_tree(project: &Path, docs_root: &str) -> Result<TreeOut, TreeError> {
    let walk = bars_core::indexing::walk::walk_docs(project, Path::new(docs_root))
        .map_err(|_| TreeError::DocsRootUnavailable)?;
    let files: Vec<TreeFile> = walk
        .files
        .iter()
        .map(|file| TreeFile {
            path: file.path.clone(),
            size: file.size,
            modified_ms: file
                .modified
                .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
                .map_or(0, |time| time.as_millis() as u64),
        })
        .collect();
    let mut hasher = blake3::Hasher::new();
    for dir in &walk.dirs {
        hasher.update(b"d\0");
        hasher.update(dir.as_bytes());
        hasher.update(b"\0");
    }
    for file in &files {
        hasher.update(b"f\0");
        hasher.update(file.path.as_bytes());
        hasher.update(&file.size.to_le_bytes());
        hasher.update(&file.modified_ms.to_le_bytes());
    }
    Ok(TreeOut {
        docs_root: docs_root.to_string(),
        files,
        dirs: walk.dirs,
        problems: walk
            .problems
            .into_iter()
            .map(|problem| (problem.path, problem.reason))
            .collect(),
        revision: hasher.finalize().to_hex().as_str()[..32].to_string(),
    })
}

/// Канонические корни и путь корня документации относительно проекта.
struct Roots {
    project: PathBuf,
    root: PathBuf,
    prefix: PathBuf,
}

fn roots(project: &Path, docs_root: &str) -> Result<Roots, TreeError> {
    let prefix: PathBuf = Path::new(&docs_root.replace('\\', "/"))
        .components()
        .filter(|part| !matches!(part, Component::CurDir))
        .collect();
    if prefix
        .components()
        .any(|part| !matches!(part, Component::Normal(_)))
    {
        return Err(TreeError::OutsideRoot);
    }
    let project = project
        .canonicalize()
        .map_err(|_| TreeError::DocsRootUnavailable)?;
    let root = project
        .join(&prefix)
        .canonicalize()
        .map_err(|_| TreeError::DocsRootUnavailable)?;
    if !root.starts_with(&project) || !root.is_dir() {
        return Err(TreeError::OutsideRoot);
    }
    Ok(Roots {
        project,
        root,
        prefix,
    })
}

impl Roots {
    /// Путь проекта (`docs/a/x.md`, корень документации — `docs` или `.`) → абсолютный путь.
    /// Узел обязан быть внутри корня документации; ссылки наружу и символические ссылки не проходят.
    /// Возвращает путь и признак «существует».
    fn locate(&self, path: &str) -> Result<(PathBuf, bool), TreeError> {
        let normalized = path.replace('\\', "/");
        let relative: PathBuf = Path::new(&normalized)
            .components()
            .filter(|part| !matches!(part, Component::CurDir))
            .collect();
        if relative
            .components()
            .any(|part| !matches!(part, Component::Normal(_)))
        {
            return Err(TreeError::InvalidPath);
        }
        if !relative.starts_with(&self.prefix) {
            return Err(TreeError::OutsideRoot);
        }
        let target = self.project.join(&relative);
        match fs::symlink_metadata(&target) {
            Ok(meta) => {
                if meta.file_type().is_symlink() {
                    return Err(TreeError::OutsideRoot);
                }
                let real = target.canonicalize().map_err(|_| TreeError::OutsideRoot)?;
                if !real.starts_with(&self.root) {
                    return Err(TreeError::OutsideRoot);
                }
                Ok((real, true))
            }
            Err(error) if error.kind() == ErrorKind::NotFound => {
                let parent = target.parent().ok_or(TreeError::InvalidPath)?;
                let real = parent
                    .canonicalize()
                    .map_err(|_| TreeError::ParentMissing)?;
                if !real.starts_with(&self.root) {
                    return Err(TreeError::OutsideRoot);
                }
                Ok((
                    real.join(target.file_name().ok_or(TreeError::InvalidPath)?),
                    false,
                ))
            }
            Err(error) => Err(io_error(&error)),
        }
    }

    fn existing_dir(&self, path: &str) -> Result<PathBuf, TreeError> {
        let (dir, exists) = self.locate(path)?;
        if !exists {
            return Err(TreeError::ParentMissing);
        }
        if !dir.is_dir() {
            return Err(TreeError::InvalidPath);
        }
        Ok(dir)
    }

    /// Путь проекта через `/` для узла внутри корня документации.
    fn project_path(&self, absolute: &Path) -> String {
        absolute
            .strip_prefix(&self.project)
            .unwrap_or(absolute)
            .components()
            .map(|part| part.as_os_str().to_string_lossy().into_owned())
            .collect::<Vec<_>>()
            .join("/")
    }
}

/// Путь без префикса `\\?\`: корзина ОС его не принимает.
fn plain(path: &Path) -> PathBuf {
    let text = path.to_string_lossy();
    match text.strip_prefix(r"\\?\") {
        Some(rest) if !rest.starts_with("UNC\\") => PathBuf::from(rest),
        _ => path.to_path_buf(),
    }
}

fn is_markdown(name: &str) -> bool {
    name.len() > 3
        && name.is_char_boundary(name.len() - 3)
        && name[name.len() - 3..].eq_ignore_ascii_case(".md")
}

const RESERVED: [&str; 22] = [
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
    "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

/// Проверяет имя нового узла и возвращает его таким, каким оно ляжет на диск: документу без
/// расширения добавляется `.md`.
pub fn check_name(name: &str, dir: bool) -> Result<String, TreeError> {
    let bad = |why: &str| Err(TreeError::InvalidName(why.to_string()));
    if name.is_empty() {
        return bad("имя пустое");
    }
    if name.chars().count() > MAX_NAME {
        return bad("длиннее 200 знаков");
    }
    if let Some(ch) = name
        .chars()
        .find(|ch| ch.is_control() || "/\\:*?\"<>|".contains(*ch))
    {
        return if ch.is_control() {
            bad("управляющий знак")
        } else {
            Err(TreeError::InvalidName(format!("знак «{ch}» запрещён")))
        };
    }
    if name.ends_with(' ') || name.ends_with('.') {
        return bad("имя не может оканчиваться пробелом или точкой");
    }
    if name.starts_with(' ') {
        return bad("имя не может начинаться с пробела");
    }
    let stem = name.split('.').next().unwrap_or(name).trim_end();
    if RESERVED.iter().any(|word| word.eq_ignore_ascii_case(stem)) {
        return bad("имя зарезервировано Windows");
    }
    if dir {
        if name.starts_with('.') {
            return bad("папка с точкой в начале невидима для индекса");
        }
        return Ok(name.to_string());
    }
    let full = if is_markdown(name) {
        name.to_string()
    } else {
        format!("{name}.md")
    };
    if full.eq_ignore_ascii_case(".md") || full.starts_with("..") {
        return bad("у документа нет имени");
    }
    Ok(full)
}

/// Имя на диске, совпадающее с `name` без учёта регистра, кроме `except`.
fn collision(dir: &Path, name: &str, except: Option<&Path>) -> Result<Option<String>, TreeError> {
    let wanted = name.to_lowercase();
    for entry in fs::read_dir(dir).map_err(|e| io_error(&e))? {
        let entry = entry.map_err(|e| io_error(&e))?;
        let found = entry.file_name().to_string_lossy().into_owned();
        if found.to_lowercase() == wanted && except.is_none_or(|skip| entry.path() != skip) {
            return Ok(Some(found));
        }
    }
    Ok(None)
}

fn yaml_title(title: &str) -> String {
    let plain_text = !title.is_empty()
        && title
            .chars()
            .all(|ch| ch.is_alphanumeric() || " -_.()".contains(ch))
        && !title.starts_with(['-', ' '])
        && title.parse::<f64>().is_err();
    if plain_text {
        title.to_string()
    } else {
        format!("\"{}\"", title.replace('\\', "\\\\").replace('"', "\\\""))
    }
}

/// Содержимое нового документа: `title` из имени и заголовок первого уровня (OQ-132).
fn template(title: &str) -> String {
    format!("---\ntitle: {}\n---\n\n# {title}\n", yaml_title(title))
}

fn stem(file_name: &str) -> &str {
    if is_markdown(file_name) {
        &file_name[..file_name.len() - 3]
    } else {
        file_name
    }
}

fn write_new(file: &Path, title: &str) -> Result<(), TreeError> {
    match document_io::write(file, &template(title), None, Eol::Lf, false) {
        Ok(WriteOutcome::Saved { .. }) => Ok(()),
        Ok(WriteOutcome::Conflict { .. }) => Err(TreeError::Exists(
            file.file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default(),
        )),
        Err(error) => Err(doc_error(error)),
    }
}

/// Создаёт документ в существующей папке; возвращает путь проекта нового файла.
pub fn create_doc(
    project: &Path,
    docs_root: &str,
    parent: &str,
    name: &str,
) -> Result<String, TreeError> {
    let roots = roots(project, docs_root)?;
    let dir = roots.existing_dir(parent)?;
    let name = check_name(name, false)?;
    if dir == roots.root && name.eq_ignore_ascii_case(INSTRUCTION_FILE) {
        return Err(TreeError::InvalidName(
            "служебный файл bars не создаётся из окна".into(),
        ));
    }
    if let Some(found) = collision(&dir, &name, None)? {
        return Err(TreeError::Exists(found));
    }
    let file = dir.join(&name);
    write_new(&file, stem(&name))?;
    Ok(roots.project_path(&file))
}

/// Итог создания папки.
#[derive(Debug, PartialEq, Eq)]
pub struct CreatedDir {
    pub path: String,
    pub index_path: String,
    /// Папка есть, а `index.md` создать не удалось.
    pub index_error: Option<TreeError>,
}

/// Создаёт папку и в ней `index.md` (имя строчными, Т-32).
pub fn create_dir(
    project: &Path,
    docs_root: &str,
    parent: &str,
    name: &str,
) -> Result<CreatedDir, TreeError> {
    let roots = roots(project, docs_root)?;
    let dir = roots.existing_dir(parent)?;
    let name = check_name(name, true)?;
    if let Some(found) = collision(&dir, &name, None)? {
        return Err(TreeError::Exists(found));
    }
    let folder = dir.join(&name);
    fs::create_dir(&folder).map_err(|e| io_error(&e))?;
    let index = folder.join("index.md");
    let index_error = write_new(&index, &name).err();
    Ok(CreatedDir {
        path: roots.project_path(&folder),
        index_path: roots.project_path(&index),
        index_error,
    })
}

/// Допуск источника переименования и удаления: существующий документ `.md` или папка без точки,
/// не корень документации и не служебный файл.
fn existing_entry(roots: &Roots, path: &str) -> Result<(PathBuf, bool), TreeError> {
    let (real, exists) = roots.locate(path)?;
    if !exists {
        return Err(TreeError::NotFound);
    }
    if real == roots.root {
        return Err(TreeError::InvalidPath);
    }
    let name = real
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let is_dir = real.is_dir();
    if is_dir {
        if name.starts_with('.') {
            return Err(TreeError::InvalidPath);
        }
    } else if !is_markdown(&name)
        || (real.parent() == Some(roots.root.as_path())
            && name.eq_ignore_ascii_case(INSTRUCTION_FILE))
    {
        return Err(TreeError::InvalidPath);
    }
    Ok((real, is_dir))
}

/// Переименовывает и перемещает файл или папку; `to_parent` — папка назначения, `name` — новое имя.
/// Возвращает новый путь проекта. Ссылки в документах не правятся (этап 3.5).
pub fn move_entry(
    project: &Path,
    docs_root: &str,
    from: &str,
    to_parent: &str,
    name: &str,
) -> Result<String, TreeError> {
    let roots = roots(project, docs_root)?;
    let (source, is_dir) = existing_entry(&roots, from)?;
    let parent = roots.existing_dir(to_parent)?;
    let name = check_name(name, is_dir)?;
    if is_dir && (parent == source || parent.starts_with(&source)) {
        return Err(TreeError::InvalidMove(
            "папку нельзя поместить внутрь неё самой".into(),
        ));
    }
    if parent == roots.root && !is_dir && name.eq_ignore_ascii_case(INSTRUCTION_FILE) {
        return Err(TreeError::InvalidName("служебное имя файла bars".into()));
    }
    let dest = parent.join(&name);
    if dest == source {
        return Err(TreeError::InvalidMove("имя и папка не изменились".into()));
    }
    let existing = collision(&parent, &name, Some(&source))?;
    if let Some(found) = existing {
        return Err(TreeError::Exists(found));
    }
    let same_folder = source.parent() == Some(parent.as_path());
    let case_only = same_folder
        && source
            .file_name()
            .is_some_and(|old| old.to_string_lossy().to_lowercase() == name.to_lowercase());
    if case_only {
        // Файловая система без учёта регистра считает `a.md` и `A.md` одним файлом: через временное имя.
        let temp = parent.join(format!(".bars-rename-{}", std::process::id()));
        fs::rename(&source, &temp).map_err(|e| io_error(&e))?;
        if let Err(error) = fs::rename(&temp, &dest) {
            let _ = fs::rename(&temp, &source);
            return Err(io_error(&error));
        }
    } else {
        fs::rename(&source, &dest).map_err(|e| io_error(&e))?;
    }
    Ok(roots.project_path(&dest))
}

/// Отправляет файл или папку в корзину ОС. Недоступная корзина — отказ, безвозвратного пути нет.
pub fn trash_entry(project: &Path, docs_root: &str, path: &str) -> Result<(), TreeError> {
    let roots = roots(project, docs_root)?;
    let (real, _) = existing_entry(&roots, path)?;
    trash::delete(plain(&real)).map_err(|error| TreeError::TrashUnavailable(error.to_string()))
}
