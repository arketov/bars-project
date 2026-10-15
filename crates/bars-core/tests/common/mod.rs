use std::fs;
use std::path::PathBuf;

/// Папка моделей bars, если в ней уже лежит bge-m3 и окружение не перекрывает кэш fastembed.
/// Тесты на настоящей модели ничего не скачивают: нет модели — они пропускаются.
pub fn cached_models_dir() -> Option<PathBuf> {
    if std::env::var_os("HF_HOME").is_some() {
        return None;
    }
    let home = std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME"))?;
    let dir = PathBuf::from(home).join(".bars").join("models");
    let snapshots = dir.join("models--BAAI--bge-m3").join("snapshots");
    let cached = fs::read_dir(snapshots)
        .ok()?
        .flatten()
        .any(|snapshot| snapshot.path().join("onnx").join("model.onnx").exists());
    cached.then_some(dir)
}
