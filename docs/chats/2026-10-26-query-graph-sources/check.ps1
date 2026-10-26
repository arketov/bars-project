param([string]$Action = "Check")
$ErrorActionPreference = 'Stop'
Set-Location (Join-Path $PSScriptRoot '../../..')
$env:PATH = "$env:USERPROFILE/.cargo/bin;$env:PATH"
$barsVswhere = Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio/Installer/vswhere.exe'
$barsVs = & $barsVswhere -latest -products '*' -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
& (Join-Path $barsVs 'Common7/Tools/Launch-VsDevShell.ps1') -Arch amd64 -HostArch amd64 -SkipAutomaticLocation
$env:VULKAN_SDK = 'C:/tools/VulkanSDK/1.4.363.0'
$env:LIB = "$env:VULKAN_SDK/Lib;$env:LIB"
$env:PATH = (Join-Path $barsVs 'Common7/IDE/CommonExtensions/Microsoft/CMake/Ninja') + ';' + $env:PATH
$env:LIBCLANG_PATH = 'C:/Program Files (x86)/Microsoft Visual Studio/2022/BuildTools/VC/Tools/Llvm/x64/bin'
$env:CARGO_TARGET_DIR = 'C:/bt/bars'
$env:RUST_TEST_THREADS = '1'
if ($Action -eq 'Review') {
    cargo clippy -j 1 --workspace --all-targets -- -D warnings
} elseif ($Action -eq 'Native') {
    npm --prefix ui run build
    if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
    cargo build -j 1 -p bars-app --example graph_lifecycle_probe
} elseif ($Action -eq 'Real') {
    cargo run -j 1 -p bars-core --example query_slice_probe -- . docs/chats/2026-10-26-query-graph-sources/real-slices.json docs/chats/2026-10-26-query-graph-sources/real-search-hybrid.json docs/chats/2026-10-26-query-graph-sources/real-search-mix.json docs/chats/2026-10-26-query-graph-sources/real-search-graph.json
} else {
    pwsh -File scripts/check.ps1 -Jobs 1
}
exit $LASTEXITCODE
