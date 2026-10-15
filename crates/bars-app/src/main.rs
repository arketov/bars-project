//! `bars`: воркер с окном и треем (Т-1). Срез 0: каркас без окон.

// Воркер подключит канал вместе с обработчиками методов; пока модуль проверяется тестами.
#[allow(dead_code, unused_imports)]
mod ipc;

fn main() -> anyhow::Result<()> {
    tauri::Builder::default().run(tauri::generate_context!())?;
    Ok(())
}
