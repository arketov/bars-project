//! Трей: иконка по состоянию воркера, меню, выход (Т-1, `docs/features/window/indexing-progress.md`).

use std::sync::Mutex;
use std::time::Duration;

use bars_worker::worker::{Health, Line, Summary};
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager};

use crate::commands::Shared;
use crate::{icons, window};

const TRAY_ID: &str = "bars";
const TICK: Duration = Duration::from_millis(600);

struct Items {
    index: MenuItem<tauri::Wry>,
    attention: MenuItem<tauri::Wry>,
    projects: MenuItem<tauri::Wry>,
    /// Проекты, на которые указывают строки состояния сейчас (ISS-43).
    targets: Mutex<Targets>,
}

/// Что в последний раз показано: иконка, подсказка и строки меню.
type Shown = (Health, bool, String, Option<Line>, Option<Line>);

#[derive(Default)]
struct Targets {
    index: Option<String>,
    attention: Option<String>,
}

/// Экран и проект, которые открывает пункт меню; `None` — пункт окна не касается.
fn route(id: &str, targets: &Targets) -> Option<(&'static str, Option<String>)> {
    match id {
        "open" => Some(("main", None)),
        "projects" => Some(("projects", None)),
        "settings" => Some(("settings", None)),
        "state-index" => Some(("indexing", targets.index.clone())),
        "state-attention" => Some(("main", targets.attention.clone())),
        _ => None,
    }
}

pub fn build(app: &AppHandle) -> tauri::Result<()> {
    let open = MenuItem::with_id(app, "open", "Открыть bars", true, None::<&str>)?;
    let index = MenuItem::with_id(app, "state-index", "Всё актуально", false, None::<&str>)?;
    let attention = MenuItem::with_id(app, "state-attention", "", false, None::<&str>)?;
    let projects = MenuItem::with_id(app, "projects", "Проекты", true, None::<&str>)?;
    let settings = MenuItem::with_id(app, "settings", "Настройки", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Выход", true, None::<&str>)?;
    let menu = Menu::with_items(
        app,
        &[
            &open,
            &PredefinedMenuItem::separator(app)?,
            &index,
            &attention,
            &PredefinedMenuItem::separator(app)?,
            &projects,
            &settings,
            &PredefinedMenuItem::separator(app)?,
            &quit,
        ],
    )?;
    TrayIconBuilder::with_id(TRAY_ID)
        .icon(icons::tray(Health::Idle, false))
        .tooltip("bars")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| {
            let id = event.id().as_ref();
            if id == "quit" {
                quit_app(app);
                return;
            }
            let target = app.try_state::<Items>().and_then(|items| {
                let targets = items.targets.lock().unwrap_or_else(|e| e.into_inner());
                route(id, &targets)
            });
            if let Some((view, project)) = target {
                window::navigate(app, view, project.as_deref());
            }
        })
        .on_tray_icon_event(|tray, event| {
            // Двойной клик по иконке открывает окно (Т-1).
            if let TrayIconEvent::DoubleClick {
                button: MouseButton::Left,
                ..
            }
            | TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                window::navigate(tray.app_handle(), "main", None);
            }
        })
        .build(app)?;
    app.manage(Items {
        index,
        attention,
        projects,
        targets: Mutex::new(Targets::default()),
    });
    Ok(())
}

/// «Выход»: воркер дописывает текущий документ, закрывает индексы и процесс завершается.
pub fn quit_app(app: &AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        // НФТ-5: окно записывает правки; отказ пользователя или ошибка записи оставляют процесс работать.
        if !crate::flush::request(&app).await {
            return;
        }
        if let Some(handler) = app.state::<Shared>().get() {
            handler.worker().shutdown().await;
        }
        app.exit(0);
    });
}

/// Следит за воркером и обновляет иконку, подсказку и строки меню. Занятая иконка мигает.
pub fn spawn_status_loop(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        let mut phase = false;
        let mut last: Option<Shown> = None;
        loop {
            tokio::time::sleep(TICK).await;
            let Some(handler) = app.state::<Shared>().get() else {
                continue;
            };
            let summary = handler.worker().summary();
            phase = !phase;
            let lit = summary.health != Health::Busy || phase;
            let state = (
                summary.health,
                lit,
                summary.tooltip.clone(),
                summary.indexing.clone(),
                summary.attention.clone(),
            );
            if last.as_ref() != Some(&state) {
                apply(&app, &summary, lit);
                last = Some(state);
            }
        }
    });
}

fn apply(app: &AppHandle, summary: &Summary, lit: bool) {
    let tray: Option<TrayIcon> = app.tray_by_id(TRAY_ID);
    if let Some(tray) = tray {
        let _ = tray.set_icon(Some(icons::tray(summary.health, lit)));
        let _ = tray.set_tooltip(Some(&summary.tooltip));
    }
    if let Some(items) = app.try_state::<Items>() {
        *items.targets.lock().unwrap_or_else(|e| e.into_inner()) = Targets {
            index: summary.indexing.as_ref().map(|line| line.project.clone()),
            attention: summary.attention.as_ref().map(|line| line.project.clone()),
        };
        let index = summary
            .indexing
            .as_ref()
            .map_or("Всё актуально", |line| line.text.as_str());
        let _ = items.index.set_text(index);
        let _ = items.index.set_enabled(summary.indexing.is_some());
        let attention = summary
            .attention
            .as_ref()
            .map_or("", |line| line.text.as_str());
        let _ = items.attention.set_text(if attention.is_empty() {
            "Нет предупреждений"
        } else {
            attention
        });
        let _ = items.attention.set_enabled(summary.attention.is_some());
        let _ = items
            .projects
            .set_text(format!("Проекты ({})", summary.projects));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn targets() -> Targets {
        Targets {
            index: Some("C:/a".into()),
            attention: Some("C:/b".into()),
        }
    }

    #[test]
    fn status_rows_open_their_own_project() {
        let t = targets();
        assert_eq!(
            route("state-index", &t),
            Some(("indexing", Some("C:/a".into())))
        );
        assert_eq!(
            route("state-attention", &t),
            Some(("main", Some("C:/b".into())))
        );
    }

    #[test]
    fn projects_row_opens_the_project_menu_without_switching() {
        assert_eq!(route("projects", &targets()), Some(("projects", None)));
        assert_eq!(route("open", &targets()), Some(("main", None)));
        assert_eq!(route("quit", &targets()), None);
    }
}
