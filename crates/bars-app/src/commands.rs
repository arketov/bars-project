//! Команды Tauri: единственная связь окна с воркером и несколько действий ОС.
//!
//! Окно и воркер — один процесс, поэтому `bars_call` вызывает обработчик методов напрямую, без канала
//! (Т-1, Т-20). Ответы и ошибки те же, что видит агент.

use std::sync::{Mutex, PoisonError};

use bars_protocol::{Call, INTERNAL_ERROR, Id, Request, RpcError};
use bars_worker::handler::WorkerHandler;
use serde_json::Value;
use tauri::{AppHandle, State};
use tauri_plugin_dialog::DialogExt;

static MOVE_OPERATION: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

#[tauri::command]
pub fn keybindings_get(state: State<'_, Shared>) -> Result<Value, String> {
    let handler = state.get().ok_or("Воркер ещё запускается")?;
    serde_json::to_value(handler.keybindings.get(handler.worker().bars_home())?)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn keybindings_save(
    app: AppHandle,
    state: State<'_, Shared>,
    platform: String,
    bindings: Value,
    expected_revision: u64,
    recovery: Option<String>,
) -> Result<Value, String> {
    use tauri::Emitter;
    let handler = state.get().ok_or("Воркер ещё запускается")?;
    let bindings = serde_json::from_value(bindings).map_err(|e| e.to_string())?;
    let snapshot = handler.keybindings.save(
        handler.worker().bars_home(),
        &platform,
        bindings,
        expected_revision,
        recovery.as_deref(),
    )?;
    let value = serde_json::to_value(snapshot).map_err(|e| e.to_string())?;
    let _ = app.emit("bars://keybindings", &value);
    Ok(value)
}

/// Обработчик воркера: появляется, когда воркер поднят, и до того окну отвечает «запускается».
#[derive(Default)]
pub struct Shared {
    pub updates: crate::window_updates::Coordinator,
    handler: Mutex<Option<WorkerHandler>>,
    pub folders: crate::folders::Targets,
    pub flush: crate::flush::Flush,
    pub documents: crate::doc_window::Targets,
}

impl Shared {
    pub fn set(&self, handler: WorkerHandler) {
        self.updates.reset();
        *self.handler.lock().unwrap_or_else(PoisonError::into_inner) = Some(handler);
    }

    pub fn get(&self) -> Option<WorkerHandler> {
        self.handler
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
}

/// Вызов метода воркера по имени и параметрам.
#[tauri::command]
pub async fn bars_call(
    window: tauri::WebviewWindow,
    state: State<'_, Shared>,
    method: String,
    mut params: Value,
) -> Result<Value, RpcError> {
    let handler = state
        .get()
        .ok_or_else(|| RpcError::standard(INTERNAL_ERROR, "воркер ещё запускается"))?;
    if method == "project_observe"
        && let Some(params) = params.as_object_mut()
    {
        params.insert("window".into(), Value::String(window.label().to_owned()));
        // Закрытие главного окна скрывает WebView, но его JS продолжает работать.
        if !window.is_visible().unwrap_or(false) {
            params.insert("project".into(), Value::Null);
        }
        if params.get("project").is_none_or(Value::is_null) {
            state.updates.release(window.label());
        }
    }
    // Служебное поле `_window` (назначение поиска, ID обращения) в протокол не попадает.
    let context = bars_worker::new_topics::SearchContext::take_from(&mut params);
    let request = Request::new(Id::Number(0), &method, params);
    let call = Call::from_request(&request)?;
    // Дерево вида worktree в кэш папок проекта не попадает: оно не дерево владельца.
    let tree_binding = if let Call::Tree(params) = &call
        && params.target.wt().is_none()
    {
        handler
            .worker()
            .resolve(
                params
                    .target
                    .project
                    .as_deref()
                    .unwrap_or(&params.target.cwd),
            )
            .ok()
            .and_then(|root| {
                handler
                    .worker()
                    .folder_identity(&root)
                    .ok()
                    .map(|identity| (root, identity))
            })
    } else {
        None
    };
    let mut result = handler.dispatch_with_context(call, context).await;
    if method == "project_observe"
        && let Ok(value) = &mut result
    {
        value["snapshot_observed"] = serde_json::json!(state.updates.observed(window.label()));
    }
    if let (Some((root, identity)), Ok(output)) = (tree_binding, &result)
        && handler.worker().folder_identity(&root).ok().as_ref() == Some(&identity)
        && let Ok(tree) = serde_json::from_value::<bars_protocol::TreeOutput>(output.clone())
    {
        state.folders.remember(root, identity, tree);
    }
    result
}

#[tauri::command]
pub fn window_observe(
    app: AppHandle,
    window: tauri::WebviewWindow,
    state: State<'_, Shared>,
    project: Option<String>,
    parts: Vec<String>,
) -> Result<Value, String> {
    use tauri::{Emitter, Manager};
    let handler = state.get().ok_or("Воркер ещё запускается")?;
    state.updates.start(
        handler.clone(),
        std::sync::Arc::new(move |label, value| {
            let _ = app.emit_to(label, "bars://project-changed", value);
        }),
        {
            let app = window.app_handle().clone();
            std::sync::Arc::new(move |label| {
                app.get_webview_window(label).is_some_and(|window| {
                    window.is_visible().unwrap_or(false) && !window.is_minimized().unwrap_or(false)
                })
            })
        },
    );
    if project.is_none()
        || !window.is_visible().unwrap_or(false)
        || window.is_minimized().unwrap_or(false)
    {
        state.updates.release(window.label());
        handler
            .worker()
            .observe_window(window.label(), None)
            .map_err(|e| e.to_string())?;
        return Ok(Value::Null);
    }
    state.updates.observe(
        &handler,
        window.label(),
        project.as_deref().unwrap_or_default(),
        crate::window_updates::parts(&parts)?,
    )
}

#[tauri::command]
pub async fn window_snapshot(
    window: tauri::WebviewWindow,
    state: State<'_, Shared>,
    subscription: String,
    known: Value,
    force: bool,
) -> Result<Value, String> {
    let handler = state.get().ok_or("Воркер ещё запускается")?;
    if !window.is_visible().unwrap_or(false) || window.is_minimized().unwrap_or(false) {
        state.updates.release(window.label());
        return Err("Окно скрыто".into());
    }
    let value = state
        .updates
        .snapshot(&handler, window.label(), &subscription, known, force)
        .await?;
    if let Some(tree) = value.pointer("/parts/index/value")
        && let Ok(tree) = serde_json::from_value::<bars_protocol::TreeOutput>(tree.clone())
        && let Some(project) = value["project"].as_str()
        && let Ok(root) = handler.worker().resolve(project)
        && let Ok(identity) = handler.worker().folder_identity(&root)
        && value["identity"].as_str() == Some(&identity)
    {
        state.folders.remember(root, identity, tree);
    }
    Ok(value)
}

/// Диалог выбора папки; `None` — пользователь отказался.
#[tauri::command]
pub async fn pick_folder(app: AppHandle) -> Result<Option<String>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        app.dialog()
            .file()
            .blocking_pick_folder()
            .and_then(|path| path.into_path().ok())
            .map(|path| path.display().to_string())
    })
    .await
    .map_err(|error| error.to_string())
}

/// Открывает файл или папку программой, связанной с ними в ОС.
#[tauri::command]
pub fn open_path(path: String) -> Result<(), String> {
    tauri_plugin_opener::open_path(path, None::<&str>).map_err(|error| error.to_string())
}

/// Редакторы из фиксированного каталога; путь исполняемого файла окно не задаёт.
#[tauri::command]
pub fn document_editors() -> Value {
    Value::Array(
        crate::documents::editors()
            .into_iter()
            .map(|editor| serde_json::json!({ "id": editor.id, "label": editor.label }))
            .collect(),
    )
}

/// Действия только с документом текущего индекса зарегистрированного проекта.
#[tauri::command]
pub async fn document_action(
    state: State<'_, Shared>,
    project: String,
    path: String,
    action: String,
    editor: Option<String>,
    wt: Option<String>,
) -> Result<String, String> {
    let handler = state.get().ok_or("воркер ещё запускается")?;
    let view = view_roots(&state, &project, wt.as_deref())?;
    let root = view.root.clone();
    let tree = handler
        .dispatch(Call::Tree(bars_protocol::TreeParams {
            target: bars_protocol::Target {
                cwd: project.clone(),
                project: Some(project),
                wt: view.wt.clone(),
            },
        }))
        .await
        .map_err(|e| e.message)?;
    let tree: bars_protocol::TreeOutput =
        serde_json::from_value(tree).map_err(|e| e.to_string())?;
    if !tree.documents.iter().any(|document| document.path == path) {
        return Err("документ отсутствует в индексе проекта; обновите дерево или выдачу".into());
    }
    let absolute = crate::documents::indexed_path(&root, &path)?;
    if action == "check" {
        match &view.wt {
            Some(name) => {
                handler
                    .worker()
                    .edit_worktree_files(&view.owner, name, vec![absolute.clone()])
            }
            None => handler
                .worker()
                .check_files(&root, vec![absolute.clone()])
                .map_err(|e| e.to_string())?,
        }
    } else {
        let file = absolute.clone();
        tauri::async_runtime::spawn_blocking(move || match action.as_str() {
            "path" => Ok(()),
            "open" => crate::documents::open(&file, editor.as_deref().unwrap_or("default")),
            "reveal" => tauri_plugin_opener::reveal_item_in_dir(&file).map_err(|e| e.to_string()),
            _ => Err("неизвестное действие документа".into()),
        })
        .await
        .map_err(|e| e.to_string())??;
    }
    Ok(absolute.display().to_string())
}

/// Окно показывает worktree (дерево, документ, поиск): первое обращение ставит построение дельты,
/// аренда окна держит слежение за W (30 секунд, окно повторяет вызов вместе с наблюдением за
/// проектом). `wt: null` или основной checkout — окно ушло с worktree, аренда снимается.
#[tauri::command]
pub fn worktree_observe(
    window: tauri::WebviewWindow,
    state: State<'_, Shared>,
    project: Option<String>,
    wt: Option<String>,
) -> Result<(), String> {
    let handler = state.get().ok_or("воркер ещё запускается")?;
    let shown = match project {
        Some(project)
            if window.is_visible().unwrap_or(false) && !window.is_minimized().unwrap_or(false) =>
        {
            let view = view_roots(&state, &project, wt.as_deref())?;
            view.wt.map(|name| (view.owner, name))
        }
        _ => None,
    };
    if let Some((owner, name)) = &shown {
        handler.worker().ensure_worktree_delta(owner, name);
    }
    handler.worker().observe_worktree_window(
        window.label(),
        shown
            .as_ref()
            .map(|(owner, name)| (owner.as_path(), name.as_str())),
    );
    Ok(())
}

/// Корень проекта и корень документации для путей редактора.
fn document_roots(state: &Shared, project: &str) -> Result<(std::path::PathBuf, String), String> {
    let handler = state.get().ok_or("воркер ещё запускается")?;
    let root = handler
        .worker()
        .resolve(project)
        .map_err(|e| e.to_string())?;
    let docs_root = handler
        .worker()
        .docs_root(&root)
        .map_err(|e| e.to_string())?;
    Ok((root, docs_root))
}

/// Вид проекта в окне: корень вида (корень проекта или worktree), корень документации владельца и,
/// для worktree, владелец с именем (`docs/features/projects/worktrees.md`, «Окно»). Допуск путей
/// документов и операций дерева считается от корня вида.
pub struct ViewRoots {
    pub root: std::path::PathBuf,
    pub docs_root: String,
    /// Зарегистрированный проект-владелец.
    pub owner: std::path::PathBuf,
    /// Имя worktree; `None` — основной checkout.
    pub wt: Option<String>,
}

/// Разрешает `wt` команды окна: пусто или `.` — основной checkout; иначе имя worktree проекта.
/// Нет такого worktree — отказ с текстом; вид без папки на диске даёт отказ чтения позже.
pub fn view_roots(state: &Shared, project: &str, wt: Option<&str>) -> Result<ViewRoots, String> {
    let (owner, docs_root) = document_roots(state, project)?;
    let wanted = wt
        .map(str::trim)
        .filter(|value| !value.is_empty() && *value != ".");
    let Some(wanted) = wanted else {
        return Ok(ViewRoots {
            root: owner.clone(),
            docs_root,
            owner,
            wt: None,
        });
    };
    let handler = state.get().ok_or("воркер ещё запускается")?;
    let found = handler
        .worker()
        .find_worktree(&owner, bars_worker::worker::WtSelector::parse(wanted))
        .map_err(|available| {
            format!(
                "у проекта нет worktree «{wanted}»; есть: {}",
                available
                    .iter()
                    .map(|item| item.name.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        })?;
    Ok(ViewRoots {
        root: found.view_root,
        docs_root,
        owner,
        wt: Some(found.name),
    })
}

fn document_failure(error: &crate::document_io::DocError) -> Value {
    serde_json::json!({ "status": "error", "kind": error.kind(), "message": error.message() })
}

/// Читает документ для редактора. Ожидаемые отказы приходят как `status: "error"` с `kind`.
#[tauri::command]
pub async fn document_read(
    state: State<'_, Shared>,
    project: String,
    path: String,
    wt: Option<String>,
) -> Result<Value, String> {
    use crate::document_io::{DocError, read, resolve};
    let view = view_roots(&state, &project, wt.as_deref())?;
    let (root, docs_root) = (view.root, view.docs_root);
    tauri::async_runtime::spawn_blocking(move || {
        let file = match resolve(&root, &docs_root, &path) {
            Ok(file) => file,
            Err(error) => return document_failure(&error),
        };
        match read(&file) {
            Ok(out) => serde_json::json!({
                "status": "ok", "text": out.text, "revision": out.revision, "eol": out.eol.name(),
                "bom": out.bom, "size": out.size, "read_only": out.read_only.map(|r| r.name()),
            }),
            // Файла ещё нет (например, `index.md` папки): пустой документ без ревизии.
            Err(DocError::NotFound) => serde_json::json!({
                "status": "ok", "text": "", "revision": Value::Null, "eol": "lf", "bom": false,
                "size": 0, "read_only": Value::Null,
            }),
            Err(error) => document_failure(&error),
        }
    })
    .await
    .map_err(|e| e.to_string())
}

/// Записывает документ, если на диске ещё ревизия `base_revision` (`null` — файла быть не должно).
#[tauri::command]
pub async fn document_write(
    state: State<'_, Shared>,
    project: String,
    path: String,
    text: String,
    base_revision: Option<String>,
    eol: String,
    bom: bool,
    wt: Option<String>,
) -> Result<Value, String> {
    use crate::document_io::{Eol, WriteOutcome, resolve, write};
    let view = view_roots(&state, &project, wt.as_deref())?;
    let (root, docs_root) = (view.root.clone(), view.docs_root.clone());
    let worker = state.get().map(|handler| handler.worker().clone());
    tauri::async_runtime::spawn_blocking(move || {
        let file = match resolve(&root, &docs_root, &path) {
            Ok(file) => file,
            Err(error) => return document_failure(&error),
        };
        match write(
            &file,
            &text,
            base_revision.as_deref(),
            Eol::parse(&eol),
            bom,
        ) {
            Ok(WriteOutcome::Saved { revision }) => {
                // Правка из окна ждёт паузы Т-41 и идёт в очередь с приоритетом правки (Т-42);
                // воркер, который не ответил, не мешает записи: файл всё равно увидит слежение.
                // В виде worktree правка идёт в очередь дельты: пересверка пути после паузы.
                if let Some(worker) = worker {
                    match &view.wt {
                        Some(name) => {
                            worker.edit_worktree_files(&view.owner, name, vec![root.join(&path)]);
                        }
                        None => {
                            let _ = worker.documents_edited(&root, vec![root.join(&path)]);
                        }
                    }
                }
                serde_json::json!({ "status": "saved", "revision": revision })
            }
            Ok(WriteOutcome::Conflict { current }) => {
                serde_json::json!({ "status": "conflict", "revision": current })
            }
            Err(error) => document_failure(&error),
        }
    })
    .await
    .map_err(|e| e.to_string())
}

/// Текущая ревизия документа на диске без передачи текста; `revision: null` — файла нет.
#[tauri::command]
pub async fn document_revision(
    state: State<'_, Shared>,
    project: String,
    path: String,
    wt: Option<String>,
) -> Result<Value, String> {
    use crate::document_io::{resolve, revision};
    let view = view_roots(&state, &project, wt.as_deref())?;
    let (root, docs_root) = (view.root, view.docs_root);
    tauri::async_runtime::spawn_blocking(move || {
        let file = match resolve(&root, &docs_root, &path) {
            Ok(file) => file,
            Err(error) => return document_failure(&error),
        };
        match revision(&file) {
            Ok(current) => serde_json::json!({ "status": "ok", "revision": current }),
            Err(error) => document_failure(&error),
        }
    })
    .await
    .map_err(|e| e.to_string())
}

/// Вложенные проекты для дерева и диалогов окна: папка проекта и область от корня внешнего.
pub fn nested_json(nested: &[bars_protocol::NestedProjectInfo]) -> Value {
    Value::Array(
        nested
            .iter()
            .map(|item| {
                serde_json::json!({
                    "name": item.name, "path": item.path, "dir": item.dir,
                    "area": item.area, "documents": item.documents,
                })
            })
            .collect(),
    )
}

/// Вложенные проекты и папки worktree внутри проекта для дерева окна: у worktree `kind: "worktree"`,
/// `name` — имя worktree, `path` — его корень W (серая пометка без галочек, `worktrees.md`).
pub fn tree_nested_json(worker: &bars_worker::worker::Worker, root: &std::path::Path) -> Value {
    let mut items = match nested_json(&worker.nested_projects(root, false)) {
        Value::Array(items) => items,
        _ => Vec::new(),
    };
    for area in bars_core::indexing::nested::areas(root) {
        if let Some(wt) = &area.worktree {
            items.push(serde_json::json!({
                "kind": "worktree", "name": wt.name, "path": area.project.display().to_string(),
                "dir": area.project_dir, "area": area.dir, "documents": 0,
            }));
        }
    }
    Value::Array(items)
}

fn tree_failure(error: &crate::tree_io::TreeError) -> Value {
    serde_json::json!({ "status": "error", "kind": error.kind(), "message": error.message() })
}

/// Дерево документации по диску (Т-33): файлы, как их видит индексатор, и папки, включая пустые.
/// С `wt` — дерево корня вида worktree по диску W: вложенных проектов и папок worktree в нём нет, а
/// первое обращение ставит построение дельты и аренду окна (`worktrees.md`, «Окно», «Открытие»).
#[tauri::command]
pub async fn tree_read(
    window: tauri::WebviewWindow,
    state: State<'_, Shared>,
    project: String,
    wt: Option<String>,
) -> Result<Value, String> {
    let view = view_roots(&state, &project, wt.as_deref())?;
    let nested = match (&view.wt, state.get()) {
        (None, Some(handler)) => tree_nested_json(handler.worker(), &view.root),
        _ => serde_json::json!([]),
    };
    if let (Some(name), Some(handler)) = (&view.wt, state.get()) {
        handler.worker().ensure_worktree_delta(&view.owner, name);
        handler
            .worker()
            .observe_worktree_window(window.label(), Some((view.owner.as_path(), name.as_str())));
    }
    let (root, docs_root) = (view.root, view.docs_root);
    tauri::async_runtime::spawn_blocking(move || {
        match crate::tree_io::read_tree(&root, &docs_root) {
            Ok(tree) => serde_json::json!({
                "status": "ok",
                "docs_root": tree.docs_root,
                "revision": tree.revision,
                "nested": nested,
                "dirs": tree.dirs,
                "files": tree.files.iter().map(|f| serde_json::json!({
                    "path": f.path, "size": f.size, "modified": f.modified_ms,
                })).collect::<Vec<_>>(),
                "problems": tree.problems.iter().map(|(path, reason)| serde_json::json!({
                    "path": path, "reason": reason,
                })).collect::<Vec<_>>(),
            }),
            Err(error) => tree_failure(&error),
        }
    })
    .await
    .map_err(|e| e.to_string())
}

/// Создаёт документ `name` в папке `parent` (путь от корня проекта).
#[tauri::command]
pub async fn entry_create_doc(
    state: State<'_, Shared>,
    project: String,
    parent: String,
    name: String,
    wt: Option<String>,
) -> Result<Value, String> {
    let view = view_roots(&state, &project, wt.as_deref())?;
    let (root, docs_root) = (view.root.clone(), view.docs_root.clone());
    // Дерево вида окно читает само; публикация по подписке относится к проекту-владельцу.
    let _publication = if view.wt.is_none() {
        state.get().map(|handler| {
            handler
                .worker()
                .window_updates()
                .after(&root, bars_worker::worker::updates::DISK)
        })
    } else {
        None
    };
    tauri::async_runtime::spawn_blocking(move || {
        match crate::tree_io::create_doc(&root, &docs_root, &parent, &name) {
            Ok(path) => serde_json::json!({ "status": "ok", "path": path }),
            Err(error) => tree_failure(&error),
        }
    })
    .await
    .map_err(|e| e.to_string())
}

/// Создаёт папку `name` в папке `parent` вместе с её `index.md`.
#[tauri::command]
pub async fn entry_create_dir(
    state: State<'_, Shared>,
    project: String,
    parent: String,
    name: String,
    wt: Option<String>,
) -> Result<Value, String> {
    let view = view_roots(&state, &project, wt.as_deref())?;
    let (root, docs_root) = (view.root.clone(), view.docs_root.clone());
    // Дерево вида окно читает само; публикация по подписке относится к проекту-владельцу.
    let _publication = if view.wt.is_none() {
        state.get().map(|handler| {
            handler
                .worker()
                .window_updates()
                .after(&root, bars_worker::worker::updates::DISK)
        })
    } else {
        None
    };
    tauri::async_runtime::spawn_blocking(move || {
        match crate::tree_io::create_dir(&root, &docs_root, &parent, &name) {
            Ok(created) => match created.index_error {
                None => serde_json::json!({
                    "status": "ok", "path": created.path, "index_path": created.index_path,
                }),
                Some(error) => serde_json::json!({
                    "status": "partial", "path": created.path, "index_path": created.index_path,
                    "kind": error.kind(), "message": error.message(),
                }),
            },
            Err(error) => tree_failure(&error),
        }
    })
    .await
    .map_err(|e| e.to_string())
}

/// Переименовывает и перемещает файл или папку: `to_parent` — папка назначения, `name` — новое имя.
#[tauri::command]
pub async fn entry_plan_move(
    app: AppHandle,
    state: State<'_, Shared>,
    project: String,
    from: String,
    to_parent: String,
    name: String,
    wt: Option<String>,
) -> Result<Value, String> {
    let _operation = MOVE_OPERATION.lock().await;
    if !crate::flush::request_clean(&app, &project).await {
        return Ok(
            serde_json::json!({"status":"error","kind":"unsaved","message":"Не все окна подтвердили запись документов. Проверьте несохранённые правки."}),
        );
    }
    let view = view_roots(&state, &project, wt.as_deref())?;
    let (root, docs_root) = (view.root, view.docs_root);
    let in_view = view.wt.is_some();
    tauri::async_runtime::spawn_blocking(move || {
        // В виде worktree индекса ссылок нет: перенос без обновления ссылок, план говорит об этом.
        let planned = if in_view {
            crate::tree_io::plan_move_without_links(&root, &docs_root, &from, &to_parent, &name)
        } else {
            crate::tree_io::plan_move(&root, &docs_root, &from, &to_parent, &name)
        };
        match planned {
            Ok(plan) => serde_json::json!({ "status": "ok", "path": plan.path, "token": plan.token,
                "links_updated": !in_view,
                "documents": plan.updates.iter().map(|u| serde_json::json!({"path":u.path,"edits":u.edits.len()})).collect::<Vec<_>>(),
                "warnings": plan.warnings.iter().map(|(path,line)| serde_json::json!({"path":path,"line":line})).collect::<Vec<_>>() }),
            Err(error) => tree_failure(&error),
        }
    }).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn entry_move(
    app: AppHandle,
    state: State<'_, Shared>,
    project: String,
    from: String,
    to_parent: String,
    name: String,
    token: String,
    wt: Option<String>,
) -> Result<Value, String> {
    let _operation = MOVE_OPERATION.lock().await;
    if !crate::flush::request_clean(&app, &project).await {
        return Ok(
            serde_json::json!({"status":"error","kind":"unsaved","message":"Не все окна подтвердили запись документов. Проверьте несохранённые правки."}),
        );
    }
    let view = view_roots(&state, &project, wt.as_deref())?;
    let (root, docs_root) = (view.root.clone(), view.docs_root.clone());
    let _publication = if view.wt.is_none() {
        state.get().map(|handler| {
            handler
                .worker()
                .window_updates()
                .after(&root, bars_worker::worker::updates::DISK)
        })
    } else {
        None
    };
    tauri::async_runtime::spawn_blocking(move || {
        // В виде worktree индекса ссылок нет: файл переносится, ссылки не обновляются.
        let moved = if view.wt.is_some() {
            crate::tree_io::move_without_links(&root, &docs_root, &from, &to_parent, &name, &token)
        } else {
            crate::tree_io::move_with_links(&root, &docs_root, &from, &to_parent, &name, &token)
        };
        match moved {
            Ok(mut result) => {
                use tauri::Emitter;
                use tauri::Manager;
                if let Some(worktree) = &view.wt {
                    // Пути затронутых документов вида — в очередь дельты; цвета и иконки у вида нет.
                    if let Some(handler) = app.state::<Shared>().get() {
                        let mut paths = Vec::new();
                        if let Ok(tree) = crate::tree_io::read_tree(&root, &docs_root) {
                            paths.extend(tree.files.into_iter().filter(|f| f.path == result.path || f.path.starts_with(&format!("{}/", result.path))).map(|f| root.join(f.path)));
                        }
                        paths.push(root.join(&result.from));
                        handler.worker().edit_worktree_files(&view.owner, worktree, paths);
                    }
                    let _ = app.emit("bars://entry-moved", serde_json::json!({ "project":project,"wt":worktree,"from":result.from,"to":result.path }));
                    return serde_json::json!({ "status": "ok", "path": result.path, "updated": result.updated, "failed": [] });
                }
                for (label, path) in app.state::<Shared>().documents.move_paths(&project, &result.from, &result.path) {
                    if let Some(window) = app.get_webview_window(&label) { let _ = window.set_title(&format!("{} — bars",path.rsplit('/').next().unwrap_or(&path))); }
                }
                if let Some(handler) = app.state::<Shared>().get() {
                    let mut paths = result.updated.iter().map(|p| root.join(p)).collect::<Vec<_>>();
                    if let Ok(tree) = crate::tree_io::read_tree(&root, &docs_root) {
                        paths.extend(tree.files.into_iter().filter(|f| f.path == result.path || f.path.starts_with(&format!("{}/",result.path))).map(|f| root.join(f.path)));
                    }
                    paths.sort(); paths.dedup();
                    let _ = handler.worker().documents_edited(&root, paths);
                }
                if let Err(error) = crate::appearance::move_keys(&root, &docs_root, &result.from, &result.path) {
                    result.failed.push((".bars/appearance.toml".into(), error.message()));
                }
                let _ = app.emit("bars://entry-moved", serde_json::json!({ "project":project,"from":result.from,"to":result.path }));
                let _ = app.emit("bars://appearance-changed", serde_json::json!({"project":project}));
                serde_json::json!({ "status": if result.failed.is_empty() {"ok"} else {"partial"}, "path": result.path,
                    "updated": result.updated, "failed": result.failed.iter().map(|(path,reason)| serde_json::json!({"path":path,"reason":reason})).collect::<Vec<_>>() })
            },
            Err(error) => tree_failure(&error),
        }
    })
    .await
    .map_err(|e| e.to_string())
}

/// Отправляет файл или папку в корзину ОС.
#[tauri::command]
pub async fn entry_trash(
    state: State<'_, Shared>,
    project: String,
    path: String,
    wt: Option<String>,
) -> Result<Value, String> {
    let view = view_roots(&state, &project, wt.as_deref())?;
    let (root, docs_root) = (view.root.clone(), view.docs_root.clone());
    // Дерево вида окно читает само; публикация по подписке относится к проекту-владельцу.
    let _publication = if view.wt.is_none() {
        state.get().map(|handler| {
            handler
                .worker()
                .window_updates()
                .after(&root, bars_worker::worker::updates::DISK)
        })
    } else {
        None
    };
    tauri::async_runtime::spawn_blocking(move || {
        match crate::tree_io::trash_entry(&root, &docs_root, &path) {
            Ok(()) => serde_json::json!({ "status": "ok" }),
            Err(error) => tree_failure(&error),
        }
    })
    .await
    .map_err(|e| e.to_string())
}

/// Перед удалением считает документы узла и входящие ссылки на них из документов, которые останутся.
/// Индекс ссылок недоступен — `links_checked: false`: удаление идёт в корзину и остаётся возможным.
#[tauri::command]
pub async fn entry_plan_delete(
    state: State<'_, Shared>,
    project: String,
    path: String,
    wt: Option<String>,
) -> Result<Value, String> {
    let view = view_roots(&state, &project, wt.as_deref())?;
    let (root, docs_root) = (view.root.clone(), view.docs_root.clone());
    let plan = {
        let (root, docs_root, path) = (root.clone(), docs_root.clone(), path.clone());
        tauri::async_runtime::spawn_blocking(move || {
            crate::tree_io::plan_delete(&root, &docs_root, &path)
        })
        .await
        .map_err(|e| e.to_string())?
    };
    let plan = match plan {
        Ok(plan) => plan,
        Err(error) => return Ok(tree_failure(&error)),
    };
    let handler = state.get().ok_or("воркер ещё запускается")?;
    // У вида worktree индекса ссылок нет: входящие ссылки не считаются (`links_checked: false`).
    let sources = if view.wt.is_some() {
        None
    } else {
        let call = Call::LinksInto(bars_protocol::LinksIntoParams {
            project: root.display().to_string(),
            paths: plan.documents.clone(),
        });
        match handler.dispatch(call).await {
            Ok(value) => serde_json::from_value::<bars_protocol::LinksIntoOutput>(value)
                .ok()
                .map(|out| out.sources),
            Err(_) => None,
        }
    };
    let mut result = serde_json::json!({
        "status": "ok",
        "folder": plan.folder,
        "documents": plan.documents.len(),
        "links_checked": sources.is_some(),
    });
    let sources = sources.unwrap_or_default();
    result["links"] = serde_json::json!(sources.iter().map(|s| s.links.len()).sum::<usize>());
    result["sources"] = serde_json::json!(sources);
    Ok(result)
}

/// Цвета и иконки проекта (Т-38). Отказы приходят как `status: "error"` с `kind`.
fn appearance_json(appearance: &crate::appearance::Appearance) -> Value {
    let section = |entries: &std::collections::BTreeMap<String, crate::appearance::Entry>| {
        entries
            .iter()
            .map(|(path, entry)| {
                (
                    path.clone(),
                    serde_json::json!({ "color": entry.color, "icon": entry.icon }),
                )
            })
            .collect::<serde_json::Map<_, _>>()
    };
    serde_json::json!({
        "status": "ok",
        "folders": section(&appearance.folders),
        "documents": section(&appearance.documents),
    })
}

fn appearance_failure(error: &crate::appearance::AppearanceError) -> Value {
    serde_json::json!({ "status": "error", "kind": error.kind(), "message": error.message() })
}

#[tauri::command]
pub async fn appearance_read(state: State<'_, Shared>, project: String) -> Result<Value, String> {
    let (root, docs_root) = document_roots(&state, &project)?;
    tauri::async_runtime::spawn_blocking(move || {
        match crate::appearance::read_for_docs(&root, &docs_root) {
            Ok(appearance) => appearance_json(&appearance),
            Err(error) => appearance_failure(&error),
        }
    })
    .await
    .map_err(|e| e.to_string())
}

/// Записывает цвет и иконку одной папки или документа; `null` сбрасывает поле.
#[tauri::command]
pub async fn appearance_set(
    app: tauri::AppHandle,
    state: State<'_, Shared>,
    project: String,
    kind: String,
    path: String,
    color: Option<String>,
    icon: Option<String>,
) -> Result<Value, String> {
    use tauri::Emitter;
    let (root, docs_root) = document_roots(&state, &project)?;
    let Some(kind) = crate::appearance::Kind::parse(&kind) else {
        return Err("неизвестный вид: нужен folder или document".into());
    };
    let value =
        tauri::async_runtime::spawn_blocking(move || {
            match crate::appearance::set_for_docs(
                &root,
                &docs_root,
                kind,
                &path,
                color.as_deref(),
                icon.as_deref(),
            ) {
                Ok(appearance) => appearance_json(&appearance),
                Err(error) => appearance_failure(&error),
            }
        })
        .await
        .map_err(|e| e.to_string())?;
    if value["status"] == "ok" {
        let _ = app.emit(
            "bars://appearance-changed",
            serde_json::json!({ "project": project }),
        );
    }
    Ok(value)
}

/// Проверяет и закрепляет цель папки при открытии меню.
#[tauri::command]
pub async fn folder_target(
    state: State<'_, Shared>,
    project: String,
    docs_root: String,
    path: String,
) -> Result<Value, String> {
    prepare_folder(&state, project, docs_root, path).await
}

pub(crate) async fn prepare_folder(
    state: &Shared,
    project: String,
    docs_root: String,
    path: String,
) -> Result<Value, String> {
    let id = state.folders.begin();
    let handler = state.get().ok_or("воркер ещё запускается")?;
    let root = handler
        .worker()
        .resolve(&project)
        .map_err(|e| e.to_string())?;
    let identity = handler
        .worker()
        .folder_identity(&root)
        .map_err(|e| e.to_string())?;
    let output = handler
        .dispatch(Call::Tree(bars_protocol::TreeParams {
            target: bars_protocol::Target {
                cwd: project.clone(),
                project: Some(project),
                wt: None,
            },
        }))
        .await
        .map_err(|e| e.message)?;
    let tree: bars_protocol::TreeOutput =
        serde_json::from_value(output).map_err(|e| e.to_string())?;
    if tree.docs_root != docs_root
        || handler
            .worker()
            .folder_identity(&root)
            .map_err(|e| e.to_string())?
            != identity
    {
        return Err("Корень документации изменился; обновите дерево".into());
    }
    let mut documents = state.folders.documents(&root, &identity, &docs_root);
    documents.extend(tree.documents);
    let target = crate::folders::target(&root, &docs_root, &path, &documents, identity)?;
    let (absolute, missing) = crate::folders::checked_path(&target)?;
    state.folders.put(id, target)?;
    Ok(serde_json::json!({ "id": id.to_string(), "absolute": absolute, "missing": missing }))
}

#[tauri::command]
pub async fn folder_action(
    state: State<'_, Shared>,
    id: String,
    action: String,
) -> Result<Value, String> {
    execute_folder(&state, id, action).await
}

pub(crate) async fn execute_folder(
    state: &Shared,
    id: String,
    action: String,
) -> Result<Value, String> {
    let target = state
        .folders
        .get(id.parse().map_err(|_| "Недопустимая цель папки")?)?;
    let handler = state.get().ok_or("воркер ещё запускается")?;
    let _guard = handler.worker().folder_action_guard().await;
    if handler
        .worker()
        .folder_identity(&target.project)
        .map_err(|e| e.to_string())?
        != target.identity
    {
        return Err("Цель устарела; откройте меню заново".into());
    }
    let (absolute, missing) = crate::folders::checked_path(&target)?;
    let mut operation_id = None;
    match action.as_str() {
        "path" => {}
        "open" => {
            if missing {
                return Err("Папка не найдена".into());
            }
            let folder = absolute.clone();
            tauri::async_runtime::spawn_blocking(move || {
                tauri_plugin_opener::open_path(folder, None::<&str>).map_err(|e| e.to_string())
            })
            .await
            .map_err(|e| e.to_string())??;
        }
        "check" => {
            let (id, _, _) = handler
                .worker()
                .reindex_receipt(&target.project, false, false)
                .map_err(|e| e.to_string())?;
            operation_id = Some(id.to_string());
        }
        _ => return Err("Неизвестное действие папки".into()),
    }
    Ok(
        serde_json::json!({ "absolute": absolute, "missing": missing, "operation_id": operation_id }),
    )
}

/// Что предложить при добавлении папки (Т-7): есть ли в ней `docs/` и какой корень отмечен.
#[tauri::command]
pub fn project_root_suggest(state: State<'_, Shared>, path: String) -> Result<Value, String> {
    let folder = std::path::Path::new(&path);
    if !folder.is_dir() {
        return Err("папка не найдена".to_string());
    }
    let suggested = bars_protocol::RootChoice::suggested(folder);
    let (inside, nested) = state
        .get()
        .map_or((Value::Null, Value::Array(Vec::new())), |handler| {
            nesting_of_folder(&handler, folder)
        });
    Ok(serde_json::json!({
        "has_docs": folder.join("docs").is_dir(),
        "suggested": if suggested == bars_protocol::RootChoice::DocsFolder { "docs" } else { "all" },
        "inside": inside,
        "nested": nested,
    }))
}

/// Вложенность новой папки среди проектов реестра (Т-7, OQ-256): в каком проекте она лежит и
/// сколько его файлов уйдёт из его индекса при каждом выборе корня; какие зарегистрированные
/// проекты лежат внутри неё и будут исключены.
fn nesting_of_folder(handler: &WorkerHandler, folder: &std::path::Path) -> (Value, Value) {
    let worker = handler.worker();
    let projects = worker.projects();
    let mut inside = Value::Null;
    if let Some(outer) = worker
        .project_for_cwd(folder)
        .filter(|outer| outer != folder)
    {
        let docs_root = worker.docs_root(&outer).unwrap_or_else(|_| ".".into());
        if let (Ok(relative), Ok(walk)) = (
            folder.strip_prefix(&outer),
            bars_core::indexing::walk::walk_docs_indexed(&outer, std::path::Path::new(&docs_root)),
        ) {
            let base = relative.to_string_lossy().replace('\\', "/");
            let count = |prefix: String| {
                walk.files
                    .iter()
                    .filter(|f| f.path.starts_with(&prefix))
                    .count()
            };
            let name = projects
                .iter()
                .find(|project| project.path == outer)
                .map_or_else(String::new, |project| project.name.clone());
            inside = serde_json::json!({
                "name": name,
                "path": outer.display().to_string(),
                "all": count(format!("{base}/")),
                "docs": count(format!("{base}/docs/")),
            });
        }
    }
    let nested = projects
        .iter()
        .filter(|project| project.path != folder && project.path.starts_with(folder))
        .map(|project| serde_json::json!({ "name": project.name, "path": project.path.display().to_string() }))
        .collect();
    (inside, Value::Array(nested))
}

/// Записывает выбранный корень в `.bars/config.toml` до регистрации (Т-7). `root` — `docs` или
/// `all`; пустой — умолчание. Настроенный проект не меняется.
#[tauri::command]
pub fn project_root_prepare(path: String, root: Option<String>) -> Result<(), String> {
    let choice = match root.as_deref() {
        None => None,
        Some(text) => Some(
            bars_protocol::RootChoice::parse(text)
                .ok_or_else(|| "корень документации: допустимо `docs` или `all`".to_string())?,
        ),
    };
    let folder = std::path::Path::new(&path);
    if !folder.is_dir() {
        return Err("папка не найдена".to_string());
    }
    bars_protocol::prepare_project(folder, choice)
        .map(|_| ())
        .map_err(|error| format!("не удалось записать настройки проекта: {error}"))
}

/// Домашняя папка bars (`~/.bars`).
#[tauri::command]
pub fn bars_dir() -> Result<String, String> {
    bars_worker::serve::bars_home()
        .map(|path| path.display().to_string())
        .map_err(|error| error.to_string())
}

/// Статистика только воркера текущего окна; произвольный путь не принимается.
#[tauri::command]
pub async fn mcp_statistics(state: State<'_, Shared>) -> Result<Value, String> {
    let handler = state.get().ok_or("воркер ещё запускается")?;
    let home = handler.worker().bars_home().to_path_buf();
    tauri::async_runtime::spawn_blocking(move || {
        let rows = bars_worker::mcp_statistics::snapshot(&home).map_err(|_| {
            "Не удалось прочитать статистику MCP. Проверьте доступ к хранилищу.".to_string()
        })?;
        serde_json::to_value(rows).map_err(|_| "Не удалось подготовить статистику MCP".to_string())
    })
    .await
    .map_err(|_| "Чтение статистики MCP прервано".to_string())?
}

/// Только журнал текущего процесса: произвольные пути из окна не принимаются.
#[tauri::command]
pub async fn mcp_history(
    state: State<'_, Shared>,
    filter: bars_worker::mcp_history::Filter,
) -> Result<Value, String> {
    let history = state.get().ok_or("воркер ещё запускается")?.mcp_history();
    tauri::async_runtime::spawn_blocking(move || {
        let page = history
            .page(&filter)
            .map_err(|_| "История MCP недоступна; проверьте хранилище".to_string())?;
        serde_json::to_value(page).map_err(|_| "Не удалось подготовить историю MCP".to_string())
    })
    .await
    .map_err(|_| "Чтение истории MCP прервано".to_string())?
}

#[tauri::command]
pub async fn mcp_history_control(
    state: State<'_, Shared>,
    enabled: Option<bool>,
    clear_confirmed: bool,
) -> Result<(), String> {
    let history = state.get().ok_or("воркер ещё запускается")?.mcp_history();
    tauri::async_runtime::spawn_blocking(move || {
        if clear_confirmed {
            history
                .clear(true)
                .map_err(|_| "История MCP не очищена".to_string())?;
        }
        if let Some(enabled) = enabled {
            history
                .set_enabled(enabled)
                .map_err(|_| "Настройка истории MCP не сохранена".to_string())?;
        }
        Ok(())
    })
    .await
    .map_err(|_| "Изменение истории MCP прервано".to_string())?
}

#[tauri::command]
pub async fn mcp_history_options(
    state: State<'_, Shared>,
    filter: bars_worker::mcp_history::Filter,
    project_query: String,
    session_query: String,
) -> Result<Value, String> {
    let history = state.get().ok_or("воркер ещё запускается")?.mcp_history();
    tauri::async_runtime::spawn_blocking(move || {
        history
            .options(&filter, &project_query, &session_query)
            .map_err(|_| "Подсказки истории MCP недоступны".to_string())
    })
    .await
    .map_err(|_| "Чтение подсказок MCP прервано".to_string())?
}

#[tauri::command]
pub async fn mcp_history_legacy(state: State<'_, Shared>) -> Result<Value, String> {
    let history = state.get().ok_or("воркер ещё запускается")?.mcp_history();
    tauri::async_runtime::spawn_blocking(move || {
        let rows = history
            .legacy()
            .map_err(|_| "Прежние агрегаты MCP недоступны".to_string())?;
        serde_json::to_value(rows).map_err(|_| "Не удалось подготовить агрегаты MCP".to_string())
    })
    .await
    .map_err(|_| "Чтение агрегатов MCP прервано".to_string())?
}

#[tauri::command]
pub fn mcp_history_diagnostics(state: State<'_, Shared>) -> Result<Value, String> {
    let history = state.get().ok_or("воркер ещё запускается")?.mcp_history();
    Ok(
        serde_json::json!({"lost_events":history.lost(),"last_storage_error":history.last_error(),"scope":"worker_admitted_calls","delivery":"unknown","retention_days":7}),
    )
}

/// Только журнал текущего процесса: произвольные пути из окна не принимаются.
#[tauri::command]
pub async fn worker_logs(level: bars_worker::logging::Level) -> Result<Value, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let snapshot = bars_worker::logging::snapshot(level).map_err(|_| {
            "Не удалось прочитать журнал воркера. Проверьте доступ к папке логов.".to_string()
        })?;
        serde_json::to_value(snapshot).map_err(|_| "Не удалось подготовить журнал".to_string())
    })
    .await
    .map_err(|_| "Чтение журнала прервано".to_string())?
}

#[cfg(test)]
mod log_tests {
    #[tokio::test]
    async fn logs_command_returns_filtered_serializable_snapshot() {
        let home = tempfile::tempdir().unwrap();
        bars_worker::logging::init(home.path()).unwrap();
        log::info!("Информация для окна");
        log::warn!("Предупреждение для окна");
        let snapshot = super::worker_logs(bars_worker::logging::Level::Warn)
            .await
            .unwrap();
        assert_eq!(snapshot["entries"].as_array().unwrap().len(), 1);
        assert_eq!(snapshot["entries"][0]["message"], "Предупреждение для окна");
        assert!(snapshot["directory"].as_str().unwrap().ends_with("logs"));
        assert!(serde_json::from_str::<bars_worker::logging::Level>("\"unknown\"").is_err());
    }
}

/// Ответ окна на просьбу записать правки перед выходом (НФТ-5): `clean`, `asking`, `proceed`, `cancel`.
#[tauri::command]
pub fn flush_reply(window: tauri::WebviewWindow, state: State<'_, Shared>, reply: String) -> bool {
    state.flush.reply(window.label(), reply)
}

/// Открывает документ в отдельном окне bars (Т-40) или выводит вперёд уже открытое.
#[tauri::command]
pub async fn document_window_open(
    app: AppHandle,
    state: State<'_, Shared>,
    project: String,
    path: String,
) -> Result<(), String> {
    let (root, docs_root) = document_roots(&state, &project)?;
    crate::document_io::resolve(&root, &docs_root, &path).map_err(|e| e.message())?;
    crate::doc_window::open(&app, &state.documents, &project, &path)
}

/// Проект и путь документа окна; `null` — окно не окно документа (главное).
#[tauri::command]
pub fn document_window_target(
    window: tauri::WebviewWindow,
    state: State<'_, Shared>,
) -> Option<Value> {
    state
        .documents
        .get(window.label())
        .map(|(project, path)| serde_json::json!({ "project": project, "path": path }))
}

/// Окно документа просит основное окно открыть документ (ссылка внутри документа): основное окно
/// показывается и получает событие `bars://open-document`.
#[tauri::command]
pub fn open_in_main_window(
    app: AppHandle,
    project: String,
    path: String,
    first_line: u32,
    last_line: u32,
) -> Result<(), String> {
    use tauri::Emitter;
    crate::window::show(&app);
    app.emit_to(
        crate::window::LABEL,
        "bars://open-document",
        serde_json::json!({
            "project": project, "path": path, "first_line": first_line, "last_line": last_line,
        }),
    )
    .map_err(|e| e.to_string())
}

/// Закрывает окно документа после записи правок; главное окно командой не закрывается.
#[tauri::command]
pub fn document_window_close(window: tauri::WebviewWindow) -> Result<(), String> {
    if !window.label().starts_with(crate::doc_window::PREFIX) {
        return Err("закрывается только окно документа".into());
    }
    window.destroy().map_err(|e| e.to_string())
}

/// Перезапускает весь процесс `bars`: окно и воркер — один процесс (OQ-57).
#[tauri::command]
pub async fn restart_app(app: AppHandle, state: State<'_, Shared>) -> Result<(), String> {
    if !crate::flush::request(&app).await {
        return Err("Перезапуск отменён: есть несохранённые правки".into());
    }
    if let Some(handler) = state.get() {
        handler.worker().shutdown().await;
    }
    app.restart()
}
