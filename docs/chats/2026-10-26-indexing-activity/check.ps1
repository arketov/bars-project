param([string]$Mode = 'all')
$ErrorActionPreference='Stop'
$env:PATH="$env:USERPROFILE/.cargo/bin;$env:PATH"
$barsVswhere=Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio/Installer/vswhere.exe'
$barsVs=& $barsVswhere -latest -products '*' -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
& (Join-Path $barsVs 'Common7/Tools/Launch-VsDevShell.ps1') -Arch amd64 -HostArch amd64 -SkipAutomaticLocation
$env:VULKAN_SDK='C:/tools/VulkanSDK/1.4.363.0'
$env:LIB="$env:VULKAN_SDK/Lib;$env:LIB"
$env:PATH=(Join-Path $barsVs 'Common7/IDE/CommonExtensions/Microsoft/CMake/Ninja')+';'+$env:PATH
$env:CARGO_TARGET_DIR='C:/bt/bars'
$env:LIBCLANG_PATH='C:/Program Files (x86)/Microsoft Visual Studio/2022/BuildTools/VC/Tools/Llvm/x64/bin'
$env:CMAKE_GENERATOR='Ninja'
$env:CMAKE_BUILD_PARALLEL_LEVEL='1'
$env:RUST_TEST_THREADS='1'
cargo fmt --all
if ($Mode -eq 'native') {
    New-Item -ItemType Directory -Force docs/chats/2026-10-26-indexing-activity/native | Out-Null
    cargo build -j 2 -p bars-app --example workspace_probe
    if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
    mt.exe -manifest crates/bars-app/examples/workspace_probe.manifest '-outputresource:C:/bt/bars/debug/examples/workspace_probe.exe;#1'
    if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
    & C:/bt/bars/debug/examples/workspace_probe.exe "$PWD/docs/chats/2026-10-26-indexing-activity/native" "$PWD/crates/bars-app/examples/indexing_activity_probe.js"
    exit $LASTEXITCODE
}
if ($Mode -eq 'all') { & pwsh -File scripts/check.ps1 -Jobs 2; exit $LASTEXITCODE }
if ($Mode -eq 'verify') {
    cargo clippy -j 2 --workspace --all-targets -- -D warnings
    if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
    cargo test -j 2 --workspace --lib --tests --bins --no-fail-fast worker::tests::activity
    exit $LASTEXITCODE
}
cargo test -j 2 --workspace --lib --tests --bins --no-fail-fast worker::
exit $LASTEXITCODE
