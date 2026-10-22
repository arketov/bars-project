param(
    [Parameter(Mandatory = $true)][string]$Output,
    [Parameter(Mandatory = $true)][string]$BarsDocs,
    [Parameter(Mandatory = $true)][string]$OverdriveDocs
)
# Проверка физического графа в WebView2 на копиях документации двух проектов. Исходные папки только читаются,
# рабочий bars не останавливается и не обновляется; изолированный профиль и воркер живут во временной папке.
$ErrorActionPreference = 'Stop'
$graphRoot = Split-Path $PSScriptRoot -Parent
$graphOutput = [IO.Path]::GetFullPath($Output)
$graphTemp = Join-Path ([IO.Path]::GetTempPath()) ('bars-graph-run-' + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $graphTemp | Out-Null
try {
    $env:PATH = (Join-Path $env:USERPROFILE '.cargo/bin') + ';' + $env:PATH
    if (-not $env:CARGO_TARGET_DIR) { $env:CARGO_TARGET_DIR = Join-Path $graphRoot 'target' }
    if (-not $env:VULKAN_SDK) { $env:VULKAN_SDK = (Get-ChildItem C:/tools/VulkanSDK,C:/VulkanSDK -Directory -ErrorAction SilentlyContinue | Sort-Object Name -Descending | Select-Object -First 1).FullName }
    $env:CMAKE_BUILD_PARALLEL_LEVEL = '1'
    $env:CMAKE_GENERATOR = 'Ninja'
    $graphVswhere = Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio/Installer/vswhere.exe'
    $graphVs = & $graphVswhere -latest -products '*' -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
    & (Join-Path $graphVs 'Common7/Tools/Launch-VsDevShell.ps1') -Arch amd64 -HostArch amd64 -SkipAutomaticLocation
    $env:LIB = (Join-Path $env:VULKAN_SDK 'Lib') + ';' + $env:LIB
    $env:PATH = (Join-Path $graphVs 'Common7/IDE/CommonExtensions/Microsoft/CMake/Ninja') + ';' + $env:PATH
    Push-Location (Join-Path $graphRoot 'ui')
    try { npm run build; if ($LASTEXITCODE) { throw 'Не собран интерфейс' } } finally { Pop-Location }
    Push-Location $graphRoot
    try {
        $graphManifest = Join-Path $graphRoot 'crates/bars-app/examples/graph_probe.manifest'
        cargo rustc --release -p bars-app --example graph_probe -- -C link-arg=/MANIFEST:EMBED -C "link-arg=/MANIFESTINPUT:$graphManifest"
        if ($LASTEXITCODE) { throw 'Не собран нативный стенд графа' }
    } finally { Pop-Location }
    $graphExe = Join-Path $graphTemp 'graph_probe.exe'
    Copy-Item -LiteralPath (Join-Path $env:CARGO_TARGET_DIR 'release/examples/graph_probe.exe') -Destination $graphExe
    Get-ChildItem (Join-Path $env:CARGO_TARGET_DIR 'release') -Filter '*.dll' | Copy-Item -Destination $graphTemp
    $graphRedist = Get-ChildItem (Join-Path $graphVs 'VC/Redist/MSVC') -Directory | Where-Object Name -Match '^\d' | Sort-Object Name -Descending | Select-Object -First 1
    Get-ChildItem (Join-Path $graphRedist.FullName 'x64') -Directory -Filter 'Microsoft.VC*.CRT' | Get-ChildItem -Filter '*.dll' | Copy-Item -Destination $graphTemp
    # Только дочерний процесс стенда: журнал и служебные пути не читают рабочий профиль.
    $env:BARS_USER_HOME = $graphTemp
    $graphProcess = Start-Process -FilePath $graphExe -ArgumentList ('"' + $graphTemp + '" "' + $BarsDocs + '" "' + $OverdriveDocs + '"') -WindowStyle Hidden -PassThru
    if (-not $graphProcess.WaitForExit(175000)) {
        if ((Get-Process -Id $graphProcess.Id).Path -eq $graphExe) { Stop-Process -Id $graphProcess.Id }
        throw 'Таймаут; остановлен только собственный стенд'
    }
    $graphResult = Join-Path $graphTemp 'result.json'
    if (-not (Test-Path $graphResult)) { throw "Нет результата, exit=$($graphProcess.ExitCode)" }
    Copy-Item -LiteralPath $graphResult -Destination $graphOutput
    $graphReport = Get-Content -LiteralPath $graphResult -Raw | ConvertFrom-Json
    Get-Content -LiteralPath $graphResult
    if (-not $graphReport.success) { throw 'Проверки WebView2 не пройдены' }
} finally {
    $graphResolved = [IO.Path]::GetFullPath($graphTemp)
    $graphAllowed = [IO.Path]::GetFullPath([IO.Path]::GetTempPath())
    if ($graphResolved.StartsWith($graphAllowed) -and (Split-Path $graphResolved -Leaf).StartsWith('bars-graph-run-')) {
        # WebView2 освобождает профиль после выхода основного процесса; повторяется только уборка.
        for ($graphAttempt = 0; $graphAttempt -lt 10; $graphAttempt++) {
            try { Remove-Item -LiteralPath $graphResolved -Recurse -Force; break }
            catch { if ($graphAttempt -eq 9) { throw }; Start-Sleep -Milliseconds 300 }
        }
    }
}
