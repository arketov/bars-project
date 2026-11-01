//! 3.5.5: нагрузка опроса окон на воркер. Настоящий обработчик, копия docs/ как проект, подделка модели.
//! `cargo run --release -p bars-worker --example bench_window_poll -- <секунд на сценарий>`.
//! Часть 1 — стоимость каждого метода опроса; часть 2 — расписание окон (то же, что `app.svelte.ts`)
//! в реальном времени с измерением CPU процесса. `tree_read` (диск, Tauri) заменён обходом `walk_docs`.
use bars_core::indexing::walk::walk_docs;
use bars_protocol::{
    ActivityParams, AuditListParams, Call, ModelsInfoParams, NewTopicsListParams,
    OperationEventsParams, ProjectObserveParams, ProjectsParams, StatusParams, Target, TreeParams,
};
use bars_worker::{
    handler::WorkerHandler,
    worker::{Worker, WorkerConfig, models::fake::FakeProvider},
};
use serde_json::json;
use std::{
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

fn cpu_ms() -> f64 {
    use windows_sys::Win32::Foundation::FILETIME;
    use windows_sys::Win32::System::Threading::{GetCurrentProcess, GetProcessTimes};
    let zero = FILETIME {
        dwLowDateTime: 0,
        dwHighDateTime: 0,
    };
    let (mut c, mut e, mut k, mut u) = (zero, zero, zero, zero);
    unsafe { GetProcessTimes(GetCurrentProcess(), &mut c, &mut e, &mut k, &mut u) };
    let t =
        |f: FILETIME| ((f.dwHighDateTime as u64) << 32 | f.dwLowDateTime as u64) as f64 / 10_000.0;
    t(k) + t(u)
}

#[derive(Clone)]
struct Ctx {
    handler: Arc<WorkerHandler>,
    root: String,
    docs: PathBuf,
    count: Arc<AtomicU64>,
}

impl Ctx {
    fn target(&self) -> Target {
        Target {
            cwd: self.root.clone(),
            project: Some(self.root.clone()),
        }
    }
    fn call_of(&self, name: &str) -> Call {
        match name {
            "activity" => Call::Activity(ActivityParams {}),
            "operation_events" => Call::OperationEvents(OperationEventsParams {
                epoch: None,
                after: None,
            }),
            "models_info" => Call::ModelsInfo(ModelsInfoParams {}),
            "projects" => Call::Projects(ProjectsParams::default()),
            "status" => Call::Status(StatusParams {
                target: self.target(),
            }),
            "tree" => Call::Tree(TreeParams {
                target: self.target(),
            }),
            "new_topics_list" => Call::NewTopicsList(NewTopicsListParams {
                project: self.root.clone(),
            }),
            "audit_list" => Call::AuditList(AuditListParams {
                project: self.root.clone(),
            }),
            "project_observe" => Call::ProjectObserve(ProjectObserveParams {
                window: "w".into(),
                project: Some(self.root.clone()),
            }),
            other => panic!("{other}"),
        }
    }
    async fn call(&self, name: &str) {
        self.count.fetch_add(1, Ordering::Relaxed);
        if name == "tree_read" {
            let (root, docs) = (PathBuf::from(&self.root), self.docs.clone());
            tokio::task::spawn_blocking(move || walk_docs(&root, &docs).unwrap().files.len())
                .await
                .unwrap();
        } else {
            self.handler.dispatch(self.call_of(name)).await.unwrap();
        }
    }
}

/// Один тик главного окна с проектом: как `refresh()` в `app.svelte.ts` (изменений нет).
async fn main_tick(ctx: &Ctx, tick: u64, hidden: bool) {
    ctx.call("activity").await;
    ctx.call("operation_events").await;
    if hidden {
        return;
    }
    if tick.is_multiple_of(5) {
        ctx.call("projects").await;
    }
    ctx.call("status").await;
    ctx.call("tree").await;
    ctx.call("tree_read").await;
    if tick.is_multiple_of(5) {
        ctx.call("new_topics_list").await;
    }
    if tick.is_multiple_of(15) {
        ctx.call("audit_list").await;
    }
}

async fn watch_loop(ctx: Ctx, stop: Arc<tokio::sync::Notify>) {
    let mut i = tokio::time::interval(Duration::from_secs(10));
    loop {
        tokio::select! { _ = i.tick() => ctx.call("project_observe").await, _ = stop.notified() => return }
    }
}

async fn main_window(ctx: Ctx, hidden: bool, stop: Arc<tokio::sync::Notify>) {
    let period = if hidden { 10 } else { 1 };
    let mut i = tokio::time::interval(Duration::from_secs(period));
    let mut tick = 0;
    loop {
        tokio::select! {
            _ = i.tick() => { tick += 1; main_tick(&ctx, tick, hidden).await }
            _ = stop.notified() => return
        }
    }
}

async fn scenario(
    ctx: &Ctx,
    name: &str,
    secs: u64,
    visible: usize,
    hidden: usize,
    doc_only: usize,
) -> serde_json::Value {
    let stop = Arc::new(tokio::sync::Notify::new());
    ctx.count.store(0, Ordering::Relaxed);
    let (c0, t0) = (cpu_ms(), Instant::now());
    let mut tasks = vec![];
    for (n, h) in std::iter::repeat_n(false, visible)
        .chain(std::iter::repeat_n(true, hidden))
        .enumerate()
    {
        let _ = n;
        tasks.push(tokio::spawn(main_window(ctx.clone(), h, stop.clone())));
    }
    for _ in 0..(visible + hidden + doc_only) {
        tasks.push(tokio::spawn(watch_loop(ctx.clone(), stop.clone())));
    }
    tokio::time::sleep(Duration::from_secs(secs)).await;
    for t in &tasks {
        t.abort();
    }
    for t in tasks {
        let _ = t.await;
    }
    let (wall, cpu) = (t0.elapsed().as_secs_f64(), cpu_ms() - c0);
    let calls = ctx.count.load(Ordering::Relaxed);
    json!({"scenario":name,"seconds":wall,"calls":calls,"calls_per_s":calls as f64/wall,"cpu_ms":cpu,"cpu_percent_of_one_core":cpu/(wall*10.0)})
}

#[tokio::main]
async fn main() {
    let secs: u64 = std::env::args()
        .nth(1)
        .and_then(|v| v.parse().ok())
        .unwrap_or(60);
    let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("project");
    let docs = PathBuf::from("docs");
    let inventory = walk_docs(&source, &docs).unwrap();
    for file in &inventory.files {
        let target = root.join(&file.path);
        std::fs::create_dir_all(target.parent().unwrap()).unwrap();
        std::fs::copy(&file.fs_path, target).unwrap();
    }
    let worker = Worker::start(
        WorkerConfig::new(temp.path().join("home")),
        FakeProvider::new("poll-bench"),
    )
    .unwrap();
    worker.register(&root).unwrap();
    let t = Instant::now();
    while !worker.is_idle() && t.elapsed() < Duration::from_secs(300) {
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    eprintln!(
        "индексация: {:.1} с, простаивает: {}",
        t.elapsed().as_secs_f64(),
        worker.is_idle()
    );
    tokio::time::sleep(Duration::from_secs(5)).await;
    let ctx = Ctx {
        handler: Arc::new(WorkerHandler::new(worker.clone())),
        root: root.display().to_string(),
        docs,
        count: Arc::new(AtomicU64::new(0)),
    };

    let mut methods = vec![];
    for name in [
        "activity",
        "operation_events",
        "status",
        "tree",
        "tree_read",
        "projects",
        "new_topics_list",
        "audit_list",
        "models_info",
        "project_observe",
    ] {
        ctx.call(name).await;
        let reps = 200;
        let c0 = cpu_ms();
        let mut times = vec![];
        for _ in 0..reps {
            let s = Instant::now();
            ctx.call(name).await;
            times.push(s.elapsed().as_secs_f64() * 1000.0);
        }
        let cpu = (cpu_ms() - c0) / reps as f64;
        times.sort_by(f64::total_cmp);
        methods.push(json!({"method":name,"p50_ms":times[reps/2],"p95_ms":times[reps*95/100],"max_ms":times[reps-1],"cpu_ms_per_call":cpu}));
    }

    let c0 = cpu_ms();
    tokio::time::sleep(Duration::from_secs(secs)).await;
    let idle = json!({"scenario":"без окон (фон воркера)","seconds":secs,"calls":0,"cpu_ms":cpu_ms()-c0,"cpu_percent_of_one_core":(cpu_ms()-c0)/(secs as f64*10.0)});
    let rows = vec![
        idle,
        scenario(&ctx, "одно видимое главное окно", secs, 1, 0, 0).await,
        scenario(&ctx, "одно спрятанное главное окно", secs, 0, 1, 0).await,
        scenario(
            &ctx,
            "четыре окна сейчас: главное + 3 окна документа",
            secs,
            1,
            0,
            3,
        )
        .await,
        scenario(
            &ctx,
            "четыре окна до 3.5.1: все опрашивают как главное",
            secs,
            4,
            0,
            0,
        )
        .await,
    ];
    worker.shutdown().await;
    println!("{}", serde_json::to_string_pretty(&json!({
        "documents": inventory.files.len(), "worker": "real handler.dispatch, FakeProvider, indexed copy of docs/",
        "tree_read": "proxy: walk_docs in spawn_blocking (Tauri read_tree not included)",
        "methods": methods, "scenarios": rows})).unwrap());
}
