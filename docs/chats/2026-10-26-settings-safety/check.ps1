param([switch]$Native)
$ErrorActionPreference = 'Stop'
$env:PATH = "$env:USERPROFILE/.cargo/bin;$env:PATH"
$barsVswhere = Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio/Installer/vswhere.exe'
$barsVs = & $barsVswhere -latest -products '*' -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
& (Join-Path $barsVs 'Common7/Tools/Launch-VsDevShell.ps1') -Arch amd64 -HostArch amd64 -SkipAutomaticLocation
$env:VULKAN_SDK = 'C:/tools/VulkanSDK/1.4.363.0'
$env:LIB = "$env:VULKAN_SDK/Lib;$env:LIB"
$env:PATH = (Join-Path $barsVs 'Common7/IDE/CommonExtensions/Microsoft/CMake/Ninja') + ';' + $env:PATH
$env:LIBCLANG_PATH = 'C:/Program Files (x86)/Microsoft Visual Studio/2022/BuildTools/VC/Tools/Llvm/x64/bin'
$env:CARGO_TARGET_DIR = 'C:/bt/bars'
$env:CMAKE_GENERATOR = 'Ninja'
$env:CMAKE_BUILD_PARALLEL_LEVEL = '1'
$env:RUST_TEST_THREADS = '1'
if ($Native) {
    npm --prefix ui run build
    if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
    $probeManifest = Join-Path $PWD 'crates/bars-app/examples/workspace_probe.manifest'
    cargo rustc -j 2 -p bars-app --example workspace_probe -- -C link-arg=/MANIFEST:EMBED -C "link-arg=/MANIFESTINPUT:$probeManifest"
} else {
    pwsh -File scripts/check.ps1 -Jobs 2
}
exit $LASTEXITCODE
