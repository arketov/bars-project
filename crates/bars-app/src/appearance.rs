//! Цвета и иконки (Т-38): файл `.bars/appearance.toml` проекта. Markdown-файлы не меняются.
//! Устройство — `docs/features/knowledge-base/colors-icons.md`.

use std::collections::BTreeMap;
use std::fs;
use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// Палитра окна: значения под тему подбирает `ui/src/theme.css` (`--pal-<id>`).
pub const COLORS: [&str; 9] = [
    "red", "orange", "amber", "green", "teal", "blue", "violet", "pink", "gray",
];

/// Встроенные иконки папок; рисует `ui/src/lib/FolderIcon.svelte`.
pub const ICONS: [&str; 20] = [
    "folder",
    "book",
    "gear",
    "code",
    "flask",
    "chat",
    "star",
    "bolt",
    "shield",
    "chart",
    "database",
    "globe",
    "users",
    "bug",
    "target",
    "rocket",
    "puzzle",
    "archive",
    "pencil",
    "magnifier",
];

/// Модуль — первый уровень от корня документации, подпапка — второй; глубже выбора нет.
pub const MAX_FOLDER_DEPTH: usize = 2;

/// Запись файла идёт целиком: чтение, правка одной записи, замена файла.
static LOCK: Mutex<()> = Mutex::new(());

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Folder,
    Document,
}

impl Kind {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "folder" => Some(Self::Folder),
            "document" => Some(Self::Document),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Entry {
    pub color: Option<String>,
    pub icon: Option<String>,
}

impl Entry {
    fn is_empty(&self) -> bool {
        self.color.is_none() && self.icon.is_none()
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Appearance {
    pub folders: BTreeMap<String, Entry>,
    pub documents: BTreeMap<String, Entry>,
}

#[derive(Debug)]
pub enum AppearanceError {
    /// Файл не разбирается: не перезаписываем, пока человек его не исправит.
    Broken(String),
    /// Путь, вид или поле не допускаются Т-38.
    NotAllowed(&'static str),
    InvalidValue(&'static str),
    Io(String),
}

impl AppearanceError {
    pub fn kind(&self) -> &'static str {
        match self {
            Self::Broken(_) => "broken",
            Self::NotAllowed(_) => "not_allowed",
            Self::InvalidValue(_) => "invalid_value",
            Self::Io(_) => "io",
        }
    }

    pub fn message(&self) -> String {
        match self {
            Self::Broken(detail) => format!("Не удалось прочитать .bars/appearance.toml: {detail}"),
            Self::NotAllowed(text) | Self::InvalidValue(text) => (*text).to_string(),
            Self::Io(detail) => format!("Не удалось записать оформление: {detail}"),
        }
    }
}

fn file_of(project: &Path) -> PathBuf {
    project.join(".bars").join("appearance.toml")
}

fn read_section(
    table: &toml::Table,
    name: &str,
) -> Result<BTreeMap<String, Entry>, AppearanceError> {
    let mut out = BTreeMap::new();
    let Some(section) = table.get(name) else {
        return Ok(out);
    };
    let section = section
        .as_table()
        .ok_or_else(|| AppearanceError::Broken(format!("[{name}] должна быть таблицей")))?;
    for (path, value) in section {
        let fields = value.as_table().ok_or_else(|| {
            AppearanceError::Broken(format!("[{name}.\"{path}\"] должна быть таблицей"))
        })?;
        let text = |key: &str| -> Result<Option<String>, AppearanceError> {
            match fields.get(key) {
                None => Ok(None),
                Some(value) => value
                    .as_str()
                    .map(|text| Some(text.to_string()))
                    .ok_or_else(|| {
                        AppearanceError::Broken(format!("{key} у «{path}» должен быть строкой"))
                    }),
            }
        };
        let entry = Entry {
            color: text("color")?,
            icon: text("icon")?,
        };
        if !entry.is_empty() {
            out.insert(path.clone(), entry);
        }
    }
    Ok(out)
}

pub fn parse(text: &str) -> Result<Appearance, AppearanceError> {
    let table: toml::Table = text
        .parse()
        .map_err(|e: toml::de::Error| AppearanceError::Broken(e.message().to_string()))?;
    Ok(Appearance {
        folders: read_section(&table, "folders")?,
        documents: read_section(&table, "documents")?,
    })
}

fn quote(text: &str) -> String {
    toml::Value::String(text.to_string()).to_string()
}

fn section_text(name: &str, entries: &BTreeMap<String, Entry>, out: &mut String) {
    let lines: Vec<String> = entries
        .iter()
        .filter_map(|(path, entry)| {
            let mut fields = Vec::new();
            if let Some(color) = &entry.color {
                fields.push(format!("color = {}", quote(color)));
            }
            if let Some(icon) = &entry.icon {
                fields.push(format!("icon = {}", quote(icon)));
            }
            (!fields.is_empty()).then(|| format!("{} = {{ {} }}\n", quote(path), fields.join(", ")))
        })
        .collect();
    if lines.is_empty() {
        return;
    }
    if !out.is_empty() {
        out.push('\n');
    }
    out.push_str(&format!("[{name}]\n"));
    out.extend(lines);
}

/// Файл в виде `[folders]` / `[documents]` со строкой на путь, как в проекте оформления.
pub fn render(appearance: &Appearance) -> String {
    let mut out = String::new();
    section_text("folders", &appearance.folders, &mut out);
    section_text("documents", &appearance.documents, &mut out);
    out
}

/// Читает оформление проекта; нет файла — пустое.
pub fn read(project: &Path) -> Result<Appearance, AppearanceError> {
    match fs::read_to_string(file_of(project)) {
        Ok(text) => parse(text.trim_start_matches('\u{feff}')),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(Appearance::default()),
        Err(error) => Err(AppearanceError::Io(error.to_string())),
    }
}

/// Путь от корня документации: прямые слеши, без пустых частей, `.` и `..`.
fn check_path(kind: Kind, path: &str) -> Result<(), AppearanceError> {
    let parts: Vec<&str> = path.split('/').collect();
    if path.is_empty()
        || path.contains('\\')
        || parts
            .iter()
            .any(|part| part.is_empty() || *part == "." || *part == "..")
    {
        return Err(AppearanceError::NotAllowed(
            "Путь должен идти от корня документации без «.» и «..»",
        ));
    }
    match kind {
        Kind::Folder if parts.len() > MAX_FOLDER_DEPTH => Err(AppearanceError::NotAllowed(
            "Цвет и иконка выбираются только у модуля и его подпапок второго уровня",
        )),
        Kind::Document if !path.to_lowercase().ends_with(".md") => Err(
            AppearanceError::NotAllowed("Цвет назначается документу .md"),
        ),
        _ => Ok(()),
    }
}

/// Записывает цвет и иконку одной записи целиком; `None` сбрасывает поле, пустая запись удаляется.
pub fn set(
    project: &Path,
    kind: Kind,
    path: &str,
    color: Option<&str>,
    icon: Option<&str>,
) -> Result<Appearance, AppearanceError> {
    check_path(kind, path)?;
    if kind == Kind::Document && icon.is_some() {
        return Err(AppearanceError::NotAllowed(
            "У документа иконки нет, только цвет",
        ));
    }
    if color.is_some_and(|value| !COLORS.contains(&value)) {
        return Err(AppearanceError::InvalidValue(
            "Такого цвета нет в палитре окна",
        ));
    }
    if icon.is_some_and(|value| !ICONS.contains(&value)) {
        return Err(AppearanceError::InvalidValue(
            "Такой иконки нет во встроенном наборе",
        ));
    }
    let _guard = LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    let mut appearance = read(project)?;
    let entries = match kind {
        Kind::Folder => &mut appearance.folders,
        Kind::Document => &mut appearance.documents,
    };
    let entry = Entry {
        color: color.map(str::to_string),
        icon: icon.map(str::to_string),
    };
    if entry.is_empty() {
        entries.remove(path);
    } else {
        entries.insert(path.to_string(), entry);
    }
    write(project, &appearance)?;
    Ok(appearance)
}

fn write(project: &Path, appearance: &Appearance) -> Result<(), AppearanceError> {
    let io = |e: std::io::Error| AppearanceError::Io(e.to_string());
    let file = file_of(project);
    let dir = file.parent().expect("у файла есть папка");
    fs::create_dir_all(dir).map_err(io)?;
    let temp = dir.join("appearance.toml.bars-tmp");
    let result = (|| {
        let mut handle = fs::File::create(&temp)?;
        handle.write_all(render(appearance).as_bytes())?;
        handle.sync_all()?;
        drop(handle);
        fs::rename(&temp, &file)
    })();
    let _ = fs::remove_file(&temp);
    result.map_err(io)
}
