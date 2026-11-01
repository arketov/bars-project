//! Отдельные окна документов (Т-40): окно bars с одним документом и редактором.
//!
//! Окно обращается к тому же воркеру, что главное. Подпись окна — хеш проекта и пути: повторное открытие
//! того же документа выводит окно вперёд, а не создаёт второе. Закрытие окна не прячет его в трей: окно
//! получает событие, записывает правки (НФТ-5) и само просит закрыть его командой `document_window_close`.

use std::collections::HashMap;
use std::sync::{Mutex, PoisonError};

use tauri::{AppHandle, Emitter, Manager, WebviewUrl, WebviewWindowBuilder, WindowEvent};

/// Префикс подписи окна документа; по нему права окна и разбор в интерфейсе.
pub const PREFIX: &str = "doc-";
/// Событие окну документа: пользователь закрывает окно, надо записать правки.
pub const CLOSE_REQUEST: &str = "bars://document-close-request";

#[derive(Default)]
pub struct Targets(Mutex<HashMap<String, (String, String)>>);

impl Targets {
    pub fn move_paths(&self, project: &str, from: &str, to: &str) -> Vec<(String, String)> {
        let mut changed = Vec::new();
        for (label, (owner, path)) in self
            .0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .iter_mut()
        {
            if owner == project {
                let next = bars_core::link_index::moves::mapped(path, from, to);
                if next != *path {
                    *path = next;
                    changed.push((label.clone(), path.clone()));
                }
            }
        }
        changed
    }

    fn label_of(&self, project: &str, path: &str) -> Option<String> {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .iter()
            .find(|(_, (owner, doc))| owner == project && doc == path)
            .map(|(label, _)| label.clone())
    }
    pub(crate) fn set(&self, label: &str, project: &str, path: &str) {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(label.to_string(), (project.to_string(), path.to_string()));
    }

    fn remove(&self, label: &str) {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(label);
    }

    /// Проект и путь документа окна `label`.
    pub fn get(&self, label: &str) -> Option<(String, String)> {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(label)
            .cloned()
    }
}

/// Подпись окна документа: одинакова для одного проекта и пути.
pub fn label_for(project: &str, path: &str) -> String {
    let hash = blake3::hash(format!("{project}\n{path}").as_bytes()).to_hex();
    format!("{PREFIX}{}", &hash[..16])
}

fn vacant_label(base: &str, occupied: impl Fn(&str) -> bool) -> String {
    let mut label = base.to_string();
    let mut suffix = 0;
    while occupied(&label) {
        suffix += 1;
        label = format!("{base}-{suffix}");
    }
    label
}

fn focus(window: &tauri::WebviewWindow) {
    let _ = window.show();
    let _ = window.unminimize();
    let _ = window.set_focus();
}

/// Открывает документ в отдельном окне или выводит вперёд уже открытое.
pub fn open(app: &AppHandle, targets: &Targets, project: &str, path: &str) -> Result<(), String> {
    let label = targets.label_of(project, path).unwrap_or_else(|| {
        vacant_label(&label_for(project, path), |id| {
            app.get_webview_window(id).is_some()
        })
    });
    if let Some(window) = app.get_webview_window(&label) {
        focus(&window);
        return Ok(());
    }
    targets.set(&label, project, path);
    let name = path.rsplit('/').next().unwrap_or(path);
    let built = WebviewWindowBuilder::new(app, &label, WebviewUrl::App("index.html".into()))
        .title(format!("{name} — bars"))
        .inner_size(960.0, 760.0)
        .min_inner_size(520.0, 400.0)
        .disable_drag_drop_handler()
        .build();
    let window = match built {
        Ok(window) => window,
        Err(error) => {
            targets.remove(&label);
            return Err(error.to_string());
        }
    };
    crate::window::configure_shortcuts(&window).map_err(|e| e.to_string())?;
    let handle = app.clone();
    let id = label.clone();
    window.on_window_event(move |event| match event {
        WindowEvent::CloseRequested { api, .. } => {
            api.prevent_close();
            let _ = handle.emit_to(&id, CLOSE_REQUEST, ());
        }
        WindowEvent::Destroyed => {
            if let Some(shared) = handle.try_state::<crate::commands::Shared>() {
                shared.documents.remove(&id);
                shared.updates.release(&id);
                if let Some(handler) = shared.get() {
                    let _ = handler.worker().observe_window(&id, None);
                }
            }
        }
        _ => {}
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn moved_windows_keep_identity_without_capturing_old_path_or_other_project() {
        let targets = Targets::default();
        targets.set("doc-a", "C:/p", "docs/a/x.md");
        targets.set("doc-b", "C:/q", "docs/a/x.md");
        assert_eq!(
            targets.move_paths("C:/p", "docs/a", "docs/b"),
            vec![("doc-a".into(), "docs/b/x.md".into())]
        );
        assert_eq!(
            targets.label_of("C:/p", "docs/b/x.md"),
            Some("doc-a".into())
        );
        assert_eq!(targets.label_of("C:/p", "docs/a/x.md"), None);
        assert_eq!(
            targets.get("doc-b"),
            Some(("C:/q".into(), "docs/a/x.md".into()))
        );
        assert_eq!(vacant_label("doc-a", |id| id == "doc-a"), "doc-a-1");
    }

    #[test]
    fn same_document_gets_the_same_window_label() {
        let a = label_for("C:/p", "docs/a.md");
        assert_eq!(a, label_for("C:/p", "docs/a.md"));
        assert_ne!(a, label_for("C:/p", "docs/b.md"));
        assert_ne!(a, label_for("C:/q", "docs/a.md"));
        assert!(a.starts_with(PREFIX) && a.len() == PREFIX.len() + 16);
    }

    #[test]
    fn targets_are_found_until_removed() {
        let targets = Targets::default();
        assert_eq!(targets.get("doc-1"), None);
        targets.set("doc-1", "C:/p", "docs/a.md");
        assert_eq!(
            targets.get("doc-1"),
            Some(("C:/p".into(), "docs/a.md".into()))
        );
        targets.remove("doc-1");
        assert_eq!(targets.get("doc-1"), None);
    }
}
