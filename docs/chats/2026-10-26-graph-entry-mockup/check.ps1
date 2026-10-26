# WatcherOnly — диагностика одного отказа; не заменяет обязательный полный scripts/check.ps1.
param([switch]$WatcherOnly, [switch]$HistoryOnly)
$ErrorActionPreference = 'Stop'
$env:PATH = "$env:USERPROFILE/.cargo/bin;$env:PATH"
$barsVswhere = Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio/Installer/vswhere.exe'
$barsVs = & $barsVswhere -latest -products '*' -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
& (Join-Path $barsVs 'Common7/Tools/Launch-VsDevShell.ps1') -Arch amd64 -HostArch amd64 -SkipAutomaticLocation
$env:VULKAN_SDK = 'C:/tools/VulkanSDK/1.4.363.0'
$env:LIB = "$env:VULKAN_SDK/Lib;$env:LIB"
$env:PATH = (Join-Path $barsVs 'Common7/IDE/CommonExtensions/Microsoft/CMake/Ninja') + ';' + $env:PATH
$env:CARGO_TARGET_DIR = 'C:/bt/bars'
$env:LIBCLANG_PATH = 'C:/Program Files (x86)/Microsoft Visual Studio/2022/BuildTools/VC/Tools/Llvm/x64/bin'
$env:RUST_TEST_THREADS = '1'
$env:CMAKE_GENERATOR = 'Ninja'
$env:CMAKE_BUILD_PARALLEL_LEVEL = '1'
if ($HistoryOnly) {
  cargo test -j 2 --workspace --lib --tests --bins --no-fail-fast own_writer_waits_without_spending_sqlite_busy_timeout -- --exact mcp_history::tests::own_writer_waits_without_spending_sqlite_busy_timeout
  exit $LASTEXITCODE
}
if ($WatcherOnly) {
  cargo test -j 2 --workspace --lib --tests --bins --no-fail-fast worker::tests::deaf_watcher_is_restarted_by_the_periodic_check -- --exact
  exit $LASTEXITCODE
}
& pwsh -File scripts/check.ps1 -Jobs 2
exit $LASTEXITCODE
