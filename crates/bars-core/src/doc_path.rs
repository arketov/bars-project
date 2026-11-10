//! Путь документа из запроса агента или CLI (`docs/features/agent-tools/mcp-tools.md`, Т-26, OQ-35,
//! OQ-169).
//!
//! Один разбор для всех входов: чтение, структура, ссылки, продолжение текстового поиска и `files`
//! поиска и графа. Принимаются обе формы пути — от корня документации и от корня проекта; чтение
//! не выходит за корень проекта.

use std::path::{Component, Path, PathBuf};

/// Путь не привёл к документу.
#[derive(Debug, PartialEq, Eq, thiserror::Error)]
pub enum DocPathError {
    /// Читаются только `.md` (`invalid_params`).
    #[error("читаются только файлы .md: {0}")]
    NotMarkdown(String),
    /// Файла нет ни по одному из допустимых отсчётов (`file_not_found`).
    #[error("файл не найден: {0}")]
    NotFound(String),
    /// Путь ведёт за корень проекта (`invalid_params`); внутри — имя проекта.
    #[error("путь вне проекта `{0}`; документ другого проекта читается с явным параметром project")]
    OutsideProject(String),
}

/// Путь после лексической нормализации: `explicit` — начинался с `./`, то есть от корня проекта.
struct Relative {
    explicit: bool,
    path: String,
}

/// Убирает `.` и `..`, `\` приравнивает к `/`. `None` — `..` выходит выше корня.
fn normalize_relative(path: &str) -> Option<Relative> {
    let unified = path.trim().replace('\\', "/");
    let explicit = unified.starts_with("./");
    let mut parts: Vec<&str> = Vec::new();
    for part in unified.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop()?;
            }
            other => parts.push(other),
        }
    }
    Some(Relative {
        explicit,
        path: parts.join("/"),
    })
}

fn is_absolute(path: &str) -> bool {
    let unified = path.trim().replace('\\', "/");
    unified.starts_with('/') || Path::new(&unified).is_absolute()
}

fn project_name(project_root: &Path) -> String {
    project_root.file_name().map_or_else(
        || project_root.display().to_string(),
        |n| n.to_string_lossy().into_owned(),
    )
}

fn has_md_extension(path: &str) -> bool {
    path.get(path.len().saturating_sub(3)..)
        .is_some_and(|ext| ext.eq_ignore_ascii_case(".md"))
}

/// Лексически сворачивает `.` и `..` абсолютного пути (файла может не быть).
fn clean_absolute(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::ParentDir => {
                out.pop();
            }
            Component::CurDir => {}
            other => out.push(other.as_os_str()),
        }
    }
    out
}

fn inside(root: &Path, candidate: &Path) -> bool {
    candidate.starts_with(root)
}

/// Находит файл документа на диске внутри корня проекта.
///
/// - абсолютный путь читается, если лежит внутри проекта;
/// - `./x.md` отсчитывается от корня проекта;
/// - остальное — от корня документации, а если там файла нет, то от корня проекта: так работает
///   и `a/x.md`, и путь из выдачи поиска `docs/a/x.md` (OQ-35);
/// - `..` нормализуется лексически, а результат (с учётом symlink и junction) обязан лежать
///   внутри корня проекта, иначе `OutsideProject` — до обращения к содержимому снаружи (OQ-169).
///
/// Разделители `\` и `/` равнозначны.
pub fn resolve_doc_path(
    project_root: &Path,
    docs_root: &Path,
    path: &str,
) -> Result<PathBuf, DocPathError> {
    let trimmed = path.trim();
    let normalized = trimmed.replace('\\', "/");
    if !has_md_extension(&normalized) {
        return Err(DocPathError::NotMarkdown(path.to_string()));
    }
    let outside = || DocPathError::OutsideProject(project_name(project_root));
    let canonical_root = project_root
        .canonicalize()
        .unwrap_or_else(|_| project_root.to_path_buf());

    let candidates: Vec<PathBuf> = if is_absolute(trimmed) {
        let absolute = PathBuf::from(&normalized);
        let known_inside = absolute
            .canonicalize()
            .map(|real| inside(&canonical_root, &real))
            .unwrap_or_else(|_| {
                let cleaned = clean_absolute(&absolute);
                inside(&canonical_root, &cleaned) || inside(&clean_absolute(project_root), &cleaned)
            });
        if !known_inside {
            return Err(outside());
        }
        vec![absolute]
    } else {
        let relative = normalize_relative(&normalized).ok_or_else(outside)?;
        if relative.explicit {
            vec![project_root.join(&relative.path)]
        } else {
            vec![
                project_root.join(docs_root).join(&relative.path),
                project_root.join(&relative.path),
            ]
        }
    };
    for candidate in candidates {
        if !candidate.is_file() {
            continue;
        }
        let real = candidate.canonicalize().map_err(|_| outside())?;
        if !inside(&canonical_root, &real) {
            return Err(outside());
        }
        return Ok(candidate);
    }
    Err(DocPathError::NotFound(path.to_string()))
}

/// Путь документа от корня проекта через `/` — в форме, в которой индекс и ссылки хранят пути.
pub fn project_relative_path(
    project_root: &Path,
    docs_root: &Path,
    path: &str,
) -> Result<String, DocPathError> {
    let file = resolve_doc_path(project_root, docs_root, path)?;
    let canonical_root = project_root
        .canonicalize()
        .unwrap_or_else(|_| project_root.to_path_buf());
    let real = file.canonicalize().unwrap_or(file);
    let relative = real
        .strip_prefix(&canonical_root)
        .map_err(|_| DocPathError::OutsideProject(project_name(project_root)))?;
    Ok(relative
        .components()
        .filter_map(|c| match c {
            Component::Normal(part) => Some(part.to_string_lossy().into_owned()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("/"))
}

fn docs_prefix(docs_root: &str) -> String {
    docs_root
        .replace('\\', "/")
        .split('/')
        .filter(|part| !matches!(*part, "" | "."))
        .collect::<Vec<_>>()
        .join("/")
}

/// Экранирование значения для `LIKE ... ESCAPE '\'` в SQLite: `%`, `_` и сама `\`.
pub fn like_escape(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for c in value.chars() {
        if matches!(c, '%' | '_' | '\\') {
            escaped.push('\\');
        }
        escaped.push(c);
    }
    escaped
}

/// Формы пути, под которыми документ может лежать в индексе (от корня проекта), по порядку
/// предпочтения: сначала корень документации, затем корень проекта. Пусто — путь выходит за корень.
pub fn stored_candidates(docs_root: &str, input: &str) -> Vec<String> {
    let Some(relative) = normalize_relative(input) else {
        return Vec::new();
    };
    let prefix = docs_prefix(docs_root);
    if relative.explicit || prefix.is_empty() {
        return vec![relative.path];
    }
    vec![format!("{prefix}/{}", relative.path), relative.path]
}

/// Путь индекса (от корня проекта) от корня документации; вне его — `./путь`.
pub fn docs_relative(docs_root: &str, stored: &str) -> String {
    let prefix = docs_prefix(docs_root);
    if prefix.is_empty() {
        return stored.to_string();
    }
    match stored.strip_prefix(&format!("{prefix}/")) {
        Some(inner) => inner.to_string(),
        None => format!("./{stored}"),
    }
}

/// До трёх документов, путь которых оканчивается на введённый или совпадает с ним по имени файла.
pub fn suggest_files<'a>(
    docs_root: &str,
    stored_paths: impl IntoIterator<Item = &'a str>,
    input: &str,
) -> Vec<String> {
    let shown: Vec<String> = stored_paths
        .into_iter()
        .map(|stored| docs_relative(docs_root, stored))
        .collect();
    crate::graph::hints::suggest_files(shown.iter().map(String::as_str), input)
}

/// Текст ошибки неизвестного файла: с кандидатами, если они есть.
pub fn unknown_file_message<'a>(
    docs_root: &str,
    stored_paths: impl IntoIterator<Item = &'a str>,
    input: &str,
) -> String {
    let shown: Vec<String> = stored_paths
        .into_iter()
        .map(|stored| docs_relative(docs_root, stored))
        .collect();
    crate::graph::hints::unknown_file_message(shown.iter().map(String::as_str), input)
}

/// Текст ошибки неизвестного модуля. `docs` — пары (модуль, путь индекса от корня проекта).
pub fn unknown_module_message(docs_root: &str, name: &str, docs: &[(String, String)]) -> String {
    let shown: Vec<(&str, String)> = docs
        .iter()
        .map(|(module, stored)| (module.as_str(), docs_relative(docs_root, stored)))
        .collect();
    let pairs: Vec<(&str, &str)> = shown.iter().map(|(m, p)| (*m, p.as_str())).collect();
    crate::graph::hints::unknown_module_message(name, &pairs)
}

#[cfg(test)]
mod tests;
