use std::process::Command;

fn main() {
    // Идентификатор сборки: коммит, из которого собраны exe. `bars`, `bars-mcp` и `bars-cli`
    // тянут этот крейт, поэтому после нового коммита пересобираются все три с одним значением.
    // Нет git или репозитория — «unknown», и сверка в `hello` молчит.
    let id = Command::new("git")
        .args(["rev-parse", "--short=12", "HEAD"])
        .output()
        .ok()
        .filter(|output| output.status.success())
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .map(|text| text.trim().to_string())
        .filter(|text| !text.is_empty())
        .unwrap_or_else(|| "unknown".into());
    println!("cargo:rustc-env=BARS_BUILD_ID={id}");
    // Журнал HEAD дополняется при каждом коммите и переключении ветки.
    println!("cargo:rerun-if-changed=../../.git/logs/HEAD");
}
