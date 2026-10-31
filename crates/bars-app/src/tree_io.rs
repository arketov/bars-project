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
    roots(project, docs_root)?;
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
        let inner = relative
            .strip_prefix(&self.prefix)
            .map_err(|_| TreeError::OutsideRoot)?;
        let parts: Vec<_> = inner.components().collect();
        // Папки с точки невидимы дереву; проверяем также предков, а не только имя цели.
        for (index, part) in parts.iter().enumerate() {
            if index + 1 < parts.len() && part.as_os_str().to_string_lossy().starts_with('.') {
                return Err(TreeError::InvalidPath);
            }
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
        if dir != self.root
            && dir
                .file_name()
                .is_some_and(|name| name.to_string_lossy().starts_with('.'))
        {
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
    if !dir && name.eq_ignore_ascii_case(".md") {
        return bad("у документа нет имени");
    }
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
        && !["true", "false", "null", "~"]
            .iter()
            .any(|word| word.eq_ignore_ascii_case(title))
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
/// Проверка выполняется и до показа плана, и перед применением.
fn move_target(
    project: &Path,
    docs_root: &str,
    from: &str,
    to_parent: &str,
    name: &str,
) -> Result<(Roots, PathBuf, PathBuf), TreeError> {
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
    Ok((roots, source, dest))
}

/// Только физическое перемещение; команда окна использует `move_with_links`.
pub fn move_entry(
    project: &Path,
    docs_root: &str,
    from: &str,
    to_parent: &str,
    name: &str,
) -> Result<String, TreeError> {
    let (roots, source, dest) = move_target(project, docs_root, from, to_parent, name)?;
    let parent = dest.parent().ok_or(TreeError::InvalidPath)?;
    let name = dest
        .file_name()
        .ok_or(TreeError::InvalidPath)?
        .to_string_lossy();
    let same_folder = source.parent() == Some(parent);
    let case_only = same_folder
        && source
            .file_name()
            .is_some_and(|old| old.to_string_lossy().to_lowercase() == name.to_lowercase());
    if case_only {
        // Файловая система без учёта регистра считает `a.md` и `A.md` одним файлом: через временное имя.
        let temp = parent.join(format!(".bars-rename-{}", std::process::id()));
        if fs::symlink_metadata(&temp).is_ok() {
            return Err(TreeError::Exists(
                temp.file_name().unwrap().to_string_lossy().into_owned(),
            ));
        }
        rename_without_replace(&source, &temp).map_err(|e| io_error(&e))?;
        if let Err(error) = rename_without_replace(&temp, &dest) {
            let _ = rename_without_replace(&temp, &source);
            return Err(io_error(&error));
        }
    } else {
        rename_without_replace(&source, &dest).map_err(|e| io_error(&e))?;
    }
    Ok(roots.project_path(&dest))
}

/// Windows не заменяет файл, появившийся между проверкой имени и перемещением.
#[cfg(windows)]
pub(crate) fn rename_without_replace(from: &Path, to: &Path) -> std::io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows::Win32::Storage::FileSystem::{MOVE_FILE_FLAGS, MoveFileExW};
    use windows::core::PCWSTR;
    let from: Vec<u16> = from.as_os_str().encode_wide().chain(Some(0)).collect();
    let to: Vec<u16> = to.as_os_str().encode_wide().chain(Some(0)).collect();
    unsafe {
        MoveFileExW(
            PCWSTR(from.as_ptr()),
            PCWSTR(to.as_ptr()),
            MOVE_FILE_FLAGS(0),
        )
    }
    .map_err(|error| std::io::Error::from_raw_os_error(error.code().0 & 0xffff))
}

#[cfg(not(windows))]
fn rename_without_replace(from: &Path, to: &Path) -> std::io::Result<()> {
    fs::rename(from, to)
}

#[derive(Debug)]
pub struct LinkUpdate {
    pub path: String,
    pub read: document_io::ReadOut,
    pub edits: Vec<bars_core::link_index::moves::Edit>,
}

#[derive(Debug)]
pub struct MovePlan {
    pub from: String,
    pub path: String,
    pub token: String,
    pub updates: Vec<LinkUpdate>,
    pub warnings: Vec<(String, u32)>,
}

/// Полный снимок перед записью. Ошибка чтения — отказ до физического перемещения.
pub fn plan_move(
    project: &Path,
    docs_root: &str,
    from: &str,
    to_parent: &str,
    name: &str,
) -> Result<MovePlan, TreeError> {
    use bars_core::link_index::{DocSet, moves};
    let (roots, source, dest) = move_target(project, docs_root, from, to_parent, name)?;
    let from = roots.project_path(&source);
    let to = roots.project_path(&dest);
    let tree = read_tree(project, docs_root)?;
    if !tree.problems.is_empty() {
        return Err(TreeError::Io(format!(
            "Нельзя составить полный план ссылок: {:?}",
            tree.problems
        )));
    }
    let prefix = docs_root
        .replace('\\', "/")
        .trim_matches('/')
        .trim_start_matches("./")
        .to_string();
    let local = |p: &str| {
        if prefix == "." || prefix.is_empty() {
            p.to_string()
        } else {
            p.strip_prefix(&format!("{prefix}/"))
                .unwrap_or(p)
                .to_string()
        }
    };
    let before = DocSet::new(tree.files.iter().map(|f| local(&f.path)));
    let after = DocSet::new(
        before
            .iter()
            .map(|p| moves::mapped(p, &local(&from), &local(&to))),
    );
    let mut files = tree.files;
    files.sort_by(|a, b| a.path.cmp(&b.path));
    let mut hash = blake3::Hasher::new();
    for value in [&from, &to] {
        hash.update(value.as_bytes());
        hash.update(b"\0");
    }
    let mut updates = Vec::new();
    let mut warnings = Vec::new();
    for file in files {
        let absolute = document_io::resolve(project, docs_root, &file.path).map_err(doc_error)?;
        let read = document_io::read(&absolute).map_err(doc_error)?;
        if read.read_only == Some(document_io::ReadOnlyReason::NotUtf8) {
            return Err(TreeError::Io(format!("{}: файл не в UTF-8", file.path)));
        }
        hash.update(file.path.as_bytes());
        hash.update(b"\0");
        hash.update(read.revision.as_bytes());
        hash.update(b"\0");
        let plan = moves::plan_with_sets(
            &read.text,
            &local(&file.path),
            &before,
            &after,
            &local(&from),
            &local(&to),
        );
        if !plan.invalid.is_empty() {
            return Err(TreeError::InvalidMove(format!(
                "{}: не удалось сохранить адресат ссылки на строках {:?}",
                file.path, plan.invalid
            )));
        }
        warnings.extend(
            plan.warnings
                .into_iter()
                .map(|line| (file.path.clone(), line)),
        );
        if !plan.edits.is_empty() {
            updates.push(LinkUpdate {
                path: moves::mapped(&file.path, &from, &to),
                read,
                edits: plan.edits,
            });
        }
    }
    Ok(MovePlan {
        from,
        path: to,
        token: hash.finalize().to_hex().to_string(),
        updates,
        warnings,
    })
}

#[derive(Debug)]
pub struct MoveResult {
    pub from: String,
    pub path: String,
    pub updated: Vec<String>,
    pub failed: Vec<(String, String)>,
}

pub fn move_with_links(
    project: &Path,
    docs_root: &str,
    from: &str,
    to_parent: &str,
    name: &str,
    token: &str,
) -> Result<MoveResult, TreeError> {
    let plan = plan_move(project, docs_root, from, to_parent, name)?;
    if plan.token != token {
        return Err(TreeError::InvalidMove(
            "документы изменились; проверьте новый план ссылок".into(),
        ));
    }
    let path = move_entry(project, docs_root, from, to_parent, name)?;
    let mut result = MoveResult {
        from: plan.from,
        path,
        updated: Vec::new(),
        failed: Vec::new(),
    };
    for update in plan.updates {
        let outcome = (|| {
            let file = document_io::resolve(project, docs_root, &update.path)?;
            let text = bars_core::link_index::moves::apply(&update.read.text, &update.edits);
            document_io::write(
                &file,
                &text,
                Some(&update.read.revision),
                update.read.eol,
                update.read.bom,
            )
        })();
        match outcome {
            Ok(WriteOutcome::Saved { .. }) => result.updated.push(update.path),
            Ok(WriteOutcome::Conflict { .. }) => result
                .failed
                .push((update.path, "Файл изменился после составления плана".into())),
            Err(error) => result.failed.push((update.path, error.message())),
        }
    }
    Ok(result)
}

/// Отправляет файл или папку в корзину ОС. Недоступная корзина — отказ, безвозвратного пути нет.
pub fn trash_entry(project: &Path, docs_root: &str, path: &str) -> Result<(), TreeError> {
    let roots = roots(project, docs_root)?;
    let (real, _) = existing_entry(&roots, path)?;
    recycle(&plain(&real)).map_err(TreeError::TrashUnavailable)
}

/// Что уйдёт в корзину: документы узла (для файла — он сам) для подсчёта входящих ссылок.
pub struct DeletePlan {
    pub folder: bool,
    /// Документы удаляемого набора, пути от корня проекта.
    pub documents: Vec<String>,
}

/// Допуск и состав набора до удаления; на диске ничего не меняет.
pub fn plan_delete(project: &Path, docs_root: &str, path: &str) -> Result<DeletePlan, TreeError> {
    let roots = roots(project, docs_root)?;
    let (real, folder) = existing_entry(&roots, path)?;
    let own = roots.project_path(&real);
    let documents = if folder {
        let inside = format!("{own}/");
        let mut files: Vec<String> = read_tree(project, docs_root)?
            .files
            .into_iter()
            .map(|file| file.path)
            .filter(|file| file.starts_with(&inside))
            .collect();
        files.sort();
        files
    } else {
        vec![own]
    };
    Ok(DeletePlan { folder, documents })
}

#[cfg(not(windows))]
fn recycle(path: &Path) -> Result<(), String> {
    trash::delete(path).map_err(|error| error.to_string())
}

#[cfg(windows)]
fn recycle(path: &Path) -> Result<(), String> {
    use std::os::windows::ffi::OsStrExt;
    use windows::Win32::System::Com::{
        CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx,
        CoUninitialize,
    };
    use windows::Win32::UI::Shell::{
        FOF_NO_UI, FOFX_EARLYFAILURE, FOFX_RECYCLEONDELETE, FileOperation, IFileOperation,
        IShellItem, SHCreateItemFromParsingName,
    };
    use windows::core::PCWSTR;

    // Собственный STA: потоки Tokio могут уже иметь несовместимую COM-модель.
    let path = path.to_owned();
    std::thread::spawn(move || {
        struct Com;
        impl Drop for Com {
            fn drop(&mut self) {
                unsafe { CoUninitialize() };
            }
        }
        let result = (|| -> windows::core::Result<()> {
            unsafe {
                CoInitializeEx(None, COINIT_APARTMENTTHREADED).ok()?;
                let _com = Com;
                let operation: IFileOperation =
                    CoCreateInstance(&FileOperation, None, CLSCTX_INPROC_SERVER)?;
                // ALLOWUNDO лишь допускает восстановление; RECYCLEONDELETE запрещает постоянное удаление.
                operation
                    .SetOperationFlags(FOF_NO_UI | FOFX_EARLYFAILURE | FOFX_RECYCLEONDELETE)?;
                let wide: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
                let item: IShellItem = SHCreateItemFromParsingName(PCWSTR(wide.as_ptr()), None)?;
                operation.DeleteItem(&item, None)?;
                operation.PerformOperations()?;
                if operation.GetAnyOperationsAborted()?.as_bool() {
                    return Err(windows::core::Error::from_hresult(windows::core::HRESULT(
                        0x800704C7u32 as i32,
                    )));
                }
            }
            Ok(())
        })();
        result.map_err(|error| error.to_string())
    })
    .join()
    .map_err(|_| "Сбой службы корзины".to_string())?
}
