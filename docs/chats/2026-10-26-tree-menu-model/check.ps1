param([ValidateSet('all','native')][string]$Mode = 'all')
$ErrorActionPreference = 'Stop'
$repo = (Resolve-Path "$PSScriptRoot/../../..").Path
Set-Location $repo
$env:PATH = "$env:USERPROFILE/.cargo/bin;$env:PATH"
$barsVswhere = Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio/Installer/vswhere.exe'
$barsVs = & $barsVswhere -latest -products '*' -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
& (Join-Path $barsVs 'Common7/Tools/Launch-VsDevShell.ps1') -Arch amd64 -HostArch amd64 -SkipAutomaticLocation
$env:VULKAN_SDK = 'C:/tools/VulkanSDK/1.4.363.0'
$env:LIB = "$env:VULKAN_SDK/Lib;$env:LIB"
$env:PATH = (Join-Path $barsVs 'Common7/IDE/CommonExtensions/Microsoft/CMake/Ninja') + ';' + $env:PATH
$env:CARGO_TARGET_DIR = 'C:/bt/bars'
$env:LIBCLANG_PATH = 'C:/Program Files (x86)/Microsoft Visual Studio/2022/BuildTools/VC/Tools/Llvm/x64/bin'
$env:CMAKE_GENERATOR = 'Ninja'
$env:CMAKE_BUILD_PARALLEL_LEVEL = '1'
$env:RUST_TEST_THREADS = '1'
if ($Mode -eq 'all') { & pwsh -File scripts/check.ps1 -Jobs 2; exit $LASTEXITCODE }
Push-Location ui
try { npm run build; $code = $LASTEXITCODE } finally { Pop-Location }
if ($code -ne 0) { exit $code }
cargo build -j 2 -p bars-app --example workspace_probe
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
$native = Join-Path $repo ('.bars/task-334/native-' + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Force $native | Out-Null
# Собственная копия: параллельные проверки могут использовать общий example.
Copy-Item C:/bt/bars/debug/examples/workspace_probe.exe "$native/probe.exe"
mt.exe -manifest crates/bars-app/examples/workspace_probe.manifest "-outputresource:$native/probe.exe;#1"
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
$nativeDeps = Get-ChildItem C:/bt/bars/debug/examples -Filter '*.dll' -File
$nativeDeps | Copy-Item -Destination $native
& "$native/probe.exe" $native "$PSScriptRoot/probe.js"
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
$result = Get-Content "$native/result.json" -Raw | ConvertFrom-Json
if (-not $result.success -or $result.checks.Count -lt 10) { throw 'Неполный результат WebView2' }
Copy-Item "$native/result.json" "$PSScriptRoot/native-result.json"
