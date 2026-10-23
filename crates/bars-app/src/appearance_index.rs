//! Оформление модуля переносится вместе с его Index.md.
use crate::appearance::{Appearance, AppearanceError, Entry};
use crate::document_io::{self, Eol, WriteOutcome};
use std::fs;
use std::path::{Path, PathBuf};

fn broken(text: &str) -> AppearanceError {
    AppearanceError::Broken(text.into())
}

fn io(error: document_io::DocError) -> AppearanceError {
    AppearanceError::Io(error.message())
}

// Поддерживаем однострочные поля, как title в индексаторе; чужие поля не сериализуем заново.
fn metadata(text: &str) -> Result<(Entry, Vec<std::ops::Range<usize>>, usize), AppearanceError> {
    let mut entry = Entry::default();
    let mut ranges = Vec::new();
    let mut lines = text.split_inclusive('\n');
    let Some(first) = lines.next() else {
        return Ok((entry, ranges, 0));
    };
    if first.trim_end() != "---" {
        return Ok((entry, ranges, 0));
    }
    let mut offset = first.len();
    let mut seen = [false; 2];
    let mut appearance_field = false;
    for line in lines {
        if matches!(line.trim_end(), "---" | "...") {
            return Ok((entry, ranges, offset));
        }
        if appearance_field
            && (line.starts_with([' ', '\t']) || line.starts_with("- "))
            && !line.trim().is_empty()
            && !line.trim_start().starts_with('#')
        {
            return Err(broken(
                "Многострочное поле оформления в Index.md не поддерживается",
            ));
        }
        if ["\"color\":", "'color':", "\"icon\":", "'icon':"]
            .iter()
            .any(|key| line.starts_with(key))
        {
            return Err(broken(
                "Имена полей color и icon в Index.md должны быть без кавычек",
            ));
        }
        if !line.trim().is_empty() && !line.trim_start().starts_with('#') {
            appearance_field = false;
        }
        for (i, key) in ["color:", "icon:"].iter().enumerate() {
            if let Some(value) = line.strip_prefix(key) {
                appearance_field = true;
                if seen[i] {
                    return Err(broken("Повтор поля оформления в Index.md"));
                }
                seen[i] = true;
                let value = value.trim();
                let value = value.split(" #").next().unwrap_or(value).trim();
                let value = value
                    .strip_prefix('"')
                    .and_then(|v| v.strip_suffix('"'))
                    .or_else(|| value.strip_prefix('\'').and_then(|v| v.strip_suffix('\'')))
                    .unwrap_or(value);
                if !value
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
                {
                    return Err(broken(
                        "color и icon в Index.md должны быть однострочными именами",
                    ));
                }
                let value = (!value.is_empty() && value != "null").then(|| value.to_owned());
                if i == 0 {
                    entry.color = value;
                } else {
                    entry.icon = value;
                }
                ranges.push(offset..offset + line.len());
            }
        }
        offset += line.len();
    }
    Err(broken("Не закрыт frontmatter в Index.md"))
}

/// Однократный перенос прежнего формата. Уже заданные поля Index.md приоритетнее.
pub fn migrate(
    project: &Path,
    docs_root: &str,
    module: &str,
    entry: &Entry,
) -> Result<bool, AppearanceError> {
    let file = match index_file(project, docs_root, module) {
        Ok(file) => file,
        Err(_) if !project.join(docs_root).join(module).is_dir() => return Ok(false),
        Err(error) => return Err(error),
    };
    match document_io::read(&file) {
        Ok(read) => {
            let (_, fields, _) = metadata(&read.text)?;
            if !fields.is_empty() {
                return Ok(true);
            }
        }
        Err(document_io::DocError::NotFound) => {}
        Err(error) => return Err(io(error)),
    }
    set(project, docs_root, module, entry)?;
    Ok(true)
}

fn index_file(project: &Path, docs_root: &str, module: &str) -> Result<PathBuf, AppearanceError> {
    let relative = Path::new(docs_root).join(module).join("Index.md");
    let relative = relative.to_string_lossy().replace('\\', "/");
    let target = document_io::resolve(project, docs_root, &relative).map_err(io)?;
    let parent = target.parent().ok_or_else(|| broken("Нет папки модуля"))?;
    let mut found = None;
    for item in fs::read_dir(parent).map_err(|e| AppearanceError::Io(e.to_string()))? {
        let item = item.map_err(|e| AppearanceError::Io(e.to_string()))?;
        if item
            .file_name()
            .to_string_lossy()
            .eq_ignore_ascii_case("index.md")
        {
            if found.is_some() {
                return Err(broken("В модуле несколько Index.md"));
            }
            let relative = Path::new(docs_root).join(module).join(item.file_name());
            found = Some(
                document_io::resolve(
                    project,
                    docs_root,
                    &relative.to_string_lossy().replace('\\', "/"),
                )
                .map_err(io)?,
            );
        }
    }
    Ok(found.unwrap_or(target))
}

pub fn read(
    project: &Path,
    docs_root: &str,
    appearance: &mut Appearance,
) -> Result<(), AppearanceError> {
    appearance.folders.retain(|key, _| key.contains('/'));
    // Допуск корня и исключение junction/symlink — общий обход дерева.
    let tree = crate::tree_io::read_tree(project, docs_root)
        .map_err(|e| AppearanceError::Io(e.message()))?;
    let prefix = docs_root.trim_matches('/').trim_start_matches("./");
    for dir in tree.dirs {
        let module = if prefix.is_empty() || prefix == "." {
            dir.as_str()
        } else {
            dir.strip_prefix(&format!("{prefix}/")).unwrap_or("")
        };
        if module.is_empty() || module.contains('/') {
            continue;
        }
        let file = index_file(project, docs_root, module)?;
        match document_io::read(&file) {
            Ok(read) => {
                if read.read_only == Some(document_io::ReadOnlyReason::NotUtf8) {
                    return Err(broken("Index.md не в UTF-8"));
                }
                let (entry, _, _) = metadata(&read.text)?;
                if entry.color.is_some() || entry.icon.is_some() {
                    appearance.folders.insert(module.to_owned(), entry);
                }
            }
            Err(document_io::DocError::NotFound) => {}
            Err(error) => return Err(io(error)),
        }
    }
    Ok(())
}

pub fn set(
    project: &Path,
    docs_root: &str,
    module: &str,
    entry: &Entry,
) -> Result<(), AppearanceError> {
    let file = index_file(project, docs_root, module)?;
    let (text, revision, eol, bom) = match document_io::read(&file) {
        Ok(read) => {
            if read.read_only.is_some() {
                return Err(broken("Index.md доступен только для чтения"));
            }
            (read.text, Some(read.revision), read.eol, read.bom)
        }
        Err(document_io::DocError::NotFound) => (
            format!(
                "---\ntitle: {}\n---\n\n# {module}\n",
                serde_json::to_string(module).unwrap()
            ),
            None,
            Eol::Lf,
            false,
        ),
        Err(error) => return Err(io(error)),
    };
    let (_, ranges, close) = metadata(&text)?;
    let mut fields = String::new();
    if let Some(color) = &entry.color {
        fields.push_str(&format!("color: {color}\n"));
    }
    if let Some(icon) = &entry.icon {
        fields.push_str(&format!("icon: {icon}\n"));
    }
    let updated = if close == 0 {
        if fields.is_empty() {
            text.clone()
        } else {
            format!("---\n{fields}---\n{text}")
        }
    } else {
        let mut header = text[..close].to_owned();
        for range in ranges.into_iter().rev() {
            header.replace_range(range, "");
        }
        format!("{header}{fields}{}", &text[close..])
    };
    match document_io::write(&file, &updated, revision.as_deref(), eol, bom).map_err(io)? {
        WriteOutcome::Saved { .. } => Ok(()),
        WriteOutcome::Conflict { .. } => Err(broken(
            "Index.md изменён другим редактором; выбор оформления не сохранён",
        )),
    }
}
