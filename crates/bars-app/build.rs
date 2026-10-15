use std::fs;
use std::path::Path;

fn main() {
    // Tauri встраивает `ui/dist` при сборке и падает, если папки нет. Без заглушки `cargo build`,
    // `clippy` и `test` не работали бы, пока не выполнен `npm run build` в `ui/`.
    let dist = Path::new("../../ui/dist");
    if !dist.join("index.html").exists() {
        fs::create_dir_all(dist).expect("не удалось создать ui/dist");
        fs::write(
            dist.join("index.html"),
            "<!doctype html><meta charset=\"utf-8\"><title>bars</title><p>Интерфейс не собран: выполните <code>npm run build</code> в папке ui.</p>",
        )
        .expect("не удалось записать заглушку ui/dist/index.html");
    }
    println!("cargo:rerun-if-changed=../../ui/dist");
    tauri_build::build();
}
