//! Действия над документом: известные редакторы и аргументы без shell.

use std::path::{Component, Path, PathBuf};

pub struct Editor {
    pub id: &'static str,
    pub label: &'static str,
    pub executable: PathBuf,
}

pub fn indexed_path(project: &Path, path: &str) -> Result<PathBuf, String> {
    let normalized = path.replace('\\', "/");
    let relative = Path::new(&normalized);
    if relative.as_os_str().is_empty()
        || !relative
            .components()
            .all(|part| matches!(part, Component::Normal(_)))
        || !relative
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("md"))
    {
        return Err("недопустимый путь документа".into());
    }
    Ok(project.join(relative))
}

pub fn editors() -> Vec<Editor> {
    let mut roots: Vec<PathBuf> = std::env::var_os("PATH")
        .map(|value| {
            std::env::split_paths(&value)
                .filter(|p| p.is_absolute())
                .collect()
        })
        .unwrap_or_default();
    for variable in ["LOCALAPPDATA", "ProgramFiles", "ProgramFiles(x86)"] {
        if let Some(root) = std::env::var_os(variable) {
            let root = PathBuf::from(root);
            roots.push(root.clone());
            roots.push(root.join("Programs"));
        }
    }
    if let Some(root) = std::env::var_os("SystemRoot") {
        roots.push(PathBuf::from(root).join("System32"));
    }
    #[cfg(target_os = "macos")]
    {
        roots.push(PathBuf::from("/Applications"));
        if let Some(home) = std::env::var_os("HOME") {
            roots.push(PathBuf::from(home).join("Applications"));
        }
    }
    let catalog: &[(&str, &str, &[&str])] = if cfg!(target_os = "windows") {
        &[
            (
                "code",
                "VS Code",
                &["Code.exe", "Microsoft VS Code/Code.exe", "../Code.exe"],
            ),
            ("notepad", "Блокнот", &["notepad.exe"]),
            ("cursor", "Cursor", &["Cursor.exe", "cursor/Cursor.exe"]),
            (
                "codium",
                "VSCodium",
                &["VSCodium.exe", "VSCodium/VSCodium.exe"],
            ),
            (
                "notepadpp",
                "Notepad++",
                &["notepad++.exe", "Notepad++/notepad++.exe"],
            ),
            (
                "sublime",
                "Sublime Text",
                &["sublime_text.exe", "Sublime Text/sublime_text.exe"],
            ),
            ("zed", "Zed", &["zed.exe", "Zed/zed.exe"]),
        ]
    } else if cfg!(target_os = "macos") {
        &[
            (
                "code",
                "VS Code",
                &["Visual Studio Code.app/Contents/MacOS/Electron", "code"],
            ),
            (
                "cursor",
                "Cursor",
                &["Cursor.app/Contents/MacOS/Cursor", "cursor"],
            ),
            (
                "codium",
                "VSCodium",
                &["VSCodium.app/Contents/MacOS/Electron", "codium"],
            ),
            (
                "sublime",
                "Sublime Text",
                &["Sublime Text.app/Contents/SharedSupport/bin/subl", "subl"],
            ),
            ("zed", "Zed", &["Zed.app/Contents/MacOS/zed", "zed"]),
        ]
    } else {
        &[
            ("code", "VS Code", &["code"]),
            ("cursor", "Cursor", &["cursor"]),
            ("codium", "VSCodium", &["codium"]),
            ("sublime", "Sublime Text", &["subl"]),
            ("zed", "Zed", &["zed"]),
        ]
    };
    catalog
        .iter()
        .filter_map(|&(id, label, names)| {
            roots
                .iter()
                .flat_map(|root| names.iter().map(move |name| root.join(name)))
                .find(|path| path.is_file())
                .map(|executable| Editor {
                    id,
                    label,
                    executable,
                })
        })
        .collect()
}

pub fn open(path: &Path, editor: &str) -> Result<(), String> {
    if !path.is_file() {
        return Err("файл не найден, возможно, его удалили".into());
    }
    if editor == "default" {
        return tauri_plugin_opener::open_path(path, None::<&str>).map_err(|e| e.to_string());
    }
    let selected = editors()
        .into_iter()
        .find(|item| item.id == editor)
        .ok_or("редактор не найден; откройте меню заново")?;
    let mut command = std::process::Command::new(selected.executable);
    command.arg(path);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000); // CREATE_NO_WINDOW: редактор сам создаёт своё окно.
    }
    command
        .spawn()
        .map(|_| ())
        .map_err(|error| format!("не удалось открыть редактор: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn document_paths_are_relative_markdown_and_preserve_spaces() {
        let root = Path::new("C:/проект с пробелами");
        assert_eq!(
            indexed_path(root, "docs/Тема файла.md").unwrap(),
            root.join("docs/Тема файла.md")
        );
        for path in [
            "../a.md",
            "docs/../../a.md",
            "/a.md",
            "C:/a.md",
            "a.exe",
            "",
        ] {
            assert!(indexed_path(root, path).is_err(), "{path}");
        }
    }
}
