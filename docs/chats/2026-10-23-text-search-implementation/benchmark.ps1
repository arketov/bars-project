$ErrorActionPreference = 'Stop'
$env:PATH = 'C:/Users/arketov/.cargo/bin;' + $env:PATH
$env:CARGO_TARGET_DIR = 'C:/bars-build'
$env:CMAKE_GENERATOR = 'Ninja'
$env:CMAKE_BUILD_PARALLEL_LEVEL = '1'
$env:VULKAN_SDK = 'C:/tools/VulkanSDK/1.4.363.0'
$vswhere = Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio/Installer/vswhere.exe'
$vsPath = & $vswhere -latest -products '*' -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
& (Join-Path $vsPath 'Common7/Tools/Launch-VsDevShell.ps1') -Arch amd64 -HostArch amd64 -SkipAutomaticLocation
$env:PATH = (Join-Path $vsPath 'Common7/IDE/CommonExtensions/Microsoft/CMake/Ninja') + ';' + $env:PATH
$env:LIB = (Join-Path $env:VULKAN_SDK 'Lib') + ';' + $env:LIB
cargo run -p bars-worker --features test-support --example bench_text_search -- C:/Users/arketov/Desktop/bars-project docs > docs/knowledge/text-search-2026-10-22/worker-2026-10-23.json
exit $LASTEXITCODE




