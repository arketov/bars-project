//! Трей: иконка по состоянию воркера, меню, выход (Т-1, `docs/features/window/indexing-progress.md`).

use std::time::Duration;

use bars_worker::worker::{Health, Summary};
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
        .on_menu_event(|app, event| match event.id().as_ref() {
            "open" | "projects" => window::navigate(app, "main", None),
            "settings" => window::navigate(app, "settings", None),
            "state-index" => window::navigate(app, "indexing", None),
            "state-attention" => window::navigate(app, "main", None),
            "quit" => quit_app(app),
            _ => {}
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
    });
    Ok(())
}

/// «Выход»: воркер дописывает текущий документ, закрывает индексы и процесс завершается.
fn quit_app(app: &AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
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
        let mut last: Option<(Health, bool, String)> = None;
        loop {
            tokio::time::sleep(TICK).await;
            let Some(handler) = app.state::<Shared>().get() else {
                continue;
            };
            let summary = handler.worker().summary();
            phase = !phase;
            let lit = summary.health != Health::Busy || phase;
            let state = (summary.health, lit, summary.tooltip.clone());
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
