param([ValidateSet('build', 'check')][string]$Action = 'build')
$ErrorActionPreference = 'Stop'
$env:PATH = "C:/Users/arketov/.cargo/bin;$env:PATH"
$env:CARGO_TARGET_DIR = 'C:/bt/bars'
$env:LIBCLANG_PATH = 'C:/Program Files (x86)/Microsoft Visual Studio/2022/BuildTools/VC/Tools/Llvm/x64/bin'
$env:VULKAN_SDK = 'C:/tools/VulkanSDK/1.4.363.0'
$env:CMAKE_GENERATOR = 'Ninja'
$env:CMAKE_BUILD_PARALLEL_LEVEL = '1'
$vswhere = Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio/Installer/vswhere.exe'
$vsPath = & $vswhere -latest -products '*' -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
& (Join-Path $vsPath 'Common7/Tools/Launch-VsDevShell.ps1') -Arch amd64 -HostArch amd64 -SkipAutomaticLocation
$env:LIB = "$env:VULKAN_SDK/Lib;$env:LIB"
$env:PATH = "$vsPath/Common7/IDE/CommonExtensions/Microsoft/CMake/Ninja;$env:PATH"
if ($Action -eq 'check') {
    $env:RUST_TEST_THREADS = '1'
    & ./scripts/check.ps1 -Jobs 2
} else {
    cargo build -j 2 --workspace --example graph_quality
}
exit $LASTEXITCODE
