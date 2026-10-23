param([Parameter(Mandatory = $true)][string]$Output)
$ErrorActionPreference = 'Stop'
$treeRoot = Split-Path $PSScriptRoot -Parent
$treeOutput = [IO.Path]::GetFullPath($Output)
$treeRun = Join-Path $treeRoot ('target/tree-probe-' + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $treeRun | Out-Null
$env:PATH = (Join-Path $env:USERPROFILE '.cargo/bin') + ';' + $env:PATH
if (-not $env:CARGO_TARGET_DIR) { $env:CARGO_TARGET_DIR = 'C:/bt/bars' }
if (-not $env:VULKAN_SDK) { $env:VULKAN_SDK = (Get-ChildItem C:/tools/VulkanSDK,C:/VulkanSDK -Directory -ErrorAction SilentlyContinue | Sort-Object Name -Descending | Select-Object -First 1).FullName }
$env:CMAKE_BUILD_PARALLEL_LEVEL = '1'
$env:CMAKE_GENERATOR = 'Ninja'
$treeVswhere = Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio/Installer/vswhere.exe'
$treeVs = & $treeVswhere -latest -products '*' -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
& (Join-Path $treeVs 'Common7/Tools/Launch-VsDevShell.ps1') -Arch amd64 -HostArch amd64 -SkipAutomaticLocation
$env:LIB = (Join-Path $env:VULKAN_SDK 'Lib') + ';' + $env:LIB
$env:PATH = (Join-Path $treeVs 'Common7/IDE/CommonExtensions/Microsoft/CMake/Ninja') + ';' + $env:PATH
Push-Location $treeRoot
try {
    $treeManifest = Join-Path $treeRoot 'crates/bars-app/examples/workspace_probe.manifest'
    cargo rustc --release -p bars-app --example tree_probe -- -C link-arg=/MANIFEST:EMBED -C "link-arg=/MANIFESTINPUT:$treeManifest"
    if ($LASTEXITCODE) { throw 'Не собран стенд дерева' }
    $treeTarget = $env:CARGO_TARGET_DIR
    Copy-Item -LiteralPath (Join-Path $treeTarget 'release/examples/tree_probe.exe') -Destination $treeRun
    Get-ChildItem (Join-Path $treeTarget 'release') -Filter '*.dll' | Copy-Item -Destination $treeRun
    $treeRedist = Get-ChildItem (Join-Path $treeVs 'VC/Redist/MSVC') -Directory | Where-Object Name -Match '^\d' | Sort-Object Name -Descending | Select-Object -First 1
    Get-ChildItem (Join-Path $treeRedist.FullName 'x64') -Directory -Filter 'Microsoft.VC*.CRT' | Get-ChildItem -Filter '*.dll' | Copy-Item -Destination $treeRun
    $treeProcess = Start-Process -FilePath (Join-Path $treeRun 'tree_probe.exe') -ArgumentList ('"' + $treeRun + '"') -WindowStyle Hidden -PassThru
    if (-not $treeProcess.WaitForExit(50000)) {
        if ((Get-Process -Id $treeProcess.Id).Path -eq (Join-Path $treeRun 'tree_probe.exe')) { Stop-Process -Id $treeProcess.Id }
        throw 'Таймаут собственного стенда дерева'
    }
    $treeResult = Join-Path $treeRun 'result.json'
    if (-not (Test-Path -LiteralPath $treeResult)) { throw "Нет результата, exit=$($treeProcess.ExitCode), стенд: $treeRun" }
    Copy-Item -LiteralPath $treeResult -Destination $treeOutput
    Get-Content -LiteralPath $treeResult
    if (-not (Get-Content -LiteralPath $treeResult -Raw | ConvertFrom-Json).success) { throw 'Проверки дерева не пройдены' }
} finally { Pop-Location }
