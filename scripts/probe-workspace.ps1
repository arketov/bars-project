param([Parameter(Mandatory = $true)][string]$Output,
      [ValidateSet('workspace_probe', 'audits_probe')][string]$Example = 'workspace_probe',
      [string]$RealModelsDir = '')
$ErrorActionPreference = 'Stop'
$workspaceRoot = Split-Path $PSScriptRoot -Parent
$workspaceOutput = [IO.Path]::GetFullPath($Output)
$workspaceTemp = Join-Path ([IO.Path]::GetTempPath()) ('bars-workspace-run-' + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $workspaceTemp | Out-Null
try {
    $env:PATH = (Join-Path $env:USERPROFILE '.cargo/bin') + ';' + $env:PATH
    if (-not $env:CARGO_TARGET_DIR) { $env:CARGO_TARGET_DIR = 'C:/bt/bars' }
    if (-not $env:VULKAN_SDK) { $env:VULKAN_SDK = (Get-ChildItem C:/tools/VulkanSDK,C:/VulkanSDK -Directory -ErrorAction SilentlyContinue | Sort-Object Name -Descending | Select-Object -First 1).FullName }
    $env:CMAKE_BUILD_PARALLEL_LEVEL = '1'
    $env:CMAKE_GENERATOR = 'Ninja'
    $workspaceVswhere = Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio/Installer/vswhere.exe'
    $workspaceVs = & $workspaceVswhere -latest -products '*' -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
    & (Join-Path $workspaceVs 'Common7/Tools/Launch-VsDevShell.ps1') -Arch amd64 -HostArch amd64 -SkipAutomaticLocation
    $env:LIB = (Join-Path $env:VULKAN_SDK 'Lib') + ';' + $env:LIB
    $env:PATH = (Join-Path $workspaceVs 'Common7/IDE/CommonExtensions/Microsoft/CMake/Ninja') + ';' + $env:PATH
    Push-Location $workspaceRoot
    try {
        $workspaceManifest = Join-Path $workspaceRoot 'crates/bars-app/examples/workspace_probe.manifest'
        cargo rustc --release -p bars-app --example $Example -- -C link-arg=/MANIFEST:EMBED -C "link-arg=/MANIFESTINPUT:$workspaceManifest"
        if ($LASTEXITCODE) { throw 'Не собран нативный стенд рабочего окна' }
    } finally { Pop-Location }
    $workspaceExe = Join-Path $workspaceTemp 'workspace_probe.exe'
    Copy-Item -LiteralPath (Join-Path $env:CARGO_TARGET_DIR "release/examples/$Example.exe") -Destination $workspaceExe
    Get-ChildItem (Join-Path $env:CARGO_TARGET_DIR 'release') -Filter '*.dll' | Copy-Item -Destination $workspaceTemp
    $workspaceRedist = Get-ChildItem (Join-Path $workspaceVs 'VC/Redist/MSVC') -Directory | Where-Object Name -Match '^\d' | Sort-Object Name -Descending | Select-Object -First 1
    Get-ChildItem (Join-Path $workspaceRedist.FullName 'x64') -Directory -Filter 'Microsoft.VC*.CRT' | Get-ChildItem -Filter '*.dll' | Copy-Item -Destination $workspaceTemp
    # Только дочерний процесс стенда: журнал и служебные пути не читают рабочий профиль.
    $env:BARS_USER_HOME = $workspaceTemp
    $workspaceArguments = '"' + $workspaceTemp + '"'
    if ($RealModelsDir) {
        if ($Example -ne 'audits_probe') { throw 'Настоящие модели поддерживает только audits_probe' }
        $workspaceArguments += ' "' + (Resolve-Path -LiteralPath $RealModelsDir).Path + '"'
    }
    $workspaceProcess = Start-Process -FilePath $workspaceExe -ArgumentList $workspaceArguments -WindowStyle Hidden -PassThru
    $workspaceDeadline = [Diagnostics.Stopwatch]::StartNew()
    $workspaceTimeout = if ($RealModelsDir) { 155 } else { 55 }
    while (-not $workspaceProcess.WaitForExit(1000)) {
        if ($workspaceDeadline.Elapsed.TotalSeconds -ge $workspaceTimeout) {
            if ((Get-Process -Id $workspaceProcess.Id).Path -eq $workspaceExe) { Stop-Process -Id $workspaceProcess.Id }
            throw 'Таймаут; остановлен только собственный стенд'
        }
    }
    $workspaceResult = Join-Path $workspaceTemp 'result.json'
    if (-not (Test-Path $workspaceResult)) { throw "Нет результата, exit=$($workspaceProcess.ExitCode)" }
    Copy-Item -LiteralPath $workspaceResult -Destination $workspaceOutput
    $workspaceReport = Get-Content -LiteralPath $workspaceResult -Raw | ConvertFrom-Json
    Get-Content -LiteralPath $workspaceResult
    if (-not $workspaceReport.success) { throw 'Проверки WebView2 не пройдены' }
} finally {
    $workspaceResolved = [IO.Path]::GetFullPath($workspaceTemp)
    $workspaceAllowed = [IO.Path]::GetFullPath([IO.Path]::GetTempPath())
    if ($workspaceResolved.StartsWith($workspaceAllowed) -and (Split-Path $workspaceResolved -Leaf).StartsWith('bars-workspace-run-')) {
        # WebView2 освобождает профиль после выхода основного процесса; повторяется только уборка.
        for ($workspaceAttempt = 0; $workspaceAttempt -lt 10; $workspaceAttempt++) {
            try { Remove-Item -LiteralPath $workspaceResolved -Recurse -Force; break }
            catch { if ($workspaceAttempt -eq 9) { throw }; Start-Sleep -Milliseconds 300 }
        }
    }
}
