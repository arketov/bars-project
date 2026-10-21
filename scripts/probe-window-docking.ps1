param(
    [Parameter(Mandatory = $true)][string]$LibclangPath,
    [Parameter(Mandatory = $true)][string]$Output
)
$ErrorActionPreference = 'Stop'
$probeRepo = Split-Path $PSScriptRoot -Parent
$probeOutput = [IO.Path]::GetFullPath($Output)
$probeTemp = Join-Path ([IO.Path]::GetTempPath()) ('bars-docking-run-' + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $probeTemp | Out-Null
try {
    if (-not (Test-Path (Join-Path $LibclangPath 'libclang.dll'))) { throw 'Нужен libclang.dll x64' }
    $env:LIBCLANG_PATH = $LibclangPath
    $env:PATH = (Join-Path $env:USERPROFILE '.cargo/bin') + ';' + $env:PATH
    if (-not $env:CARGO_TARGET_DIR) { $env:CARGO_TARGET_DIR = Join-Path $probeRepo 'target' }
    if (-not $env:VULKAN_SDK) {
        $env:VULKAN_SDK = (Get-ChildItem C:/tools/VulkanSDK -Directory | Sort-Object Name -Descending | Select-Object -First 1).FullName
    }
    $env:CMAKE_BUILD_PARALLEL_LEVEL = '1'
    $env:CMAKE_GENERATOR = 'Ninja'
    $probeVswhere = Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio/Installer/vswhere.exe'
    $probeVs = & $probeVswhere -latest -products '*' -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
    & (Join-Path $probeVs 'Common7/Tools/Launch-VsDevShell.ps1') -Arch amd64 -HostArch amd64 -SkipAutomaticLocation
    $env:PATH = (Join-Path $probeVs 'Common7/IDE/CommonExtensions/Microsoft/CMake/Ninja') + ';' + $env:PATH
    Push-Location (Join-Path $probeRepo 'ui')
    try { npm run probe:docking:build; if ($LASTEXITCODE) { throw 'Не собран интерфейс стенда' } }
    finally { Pop-Location }
    $probeManifest = Join-Path $probeRepo 'crates/bars-app/examples/docking_probe.manifest'
    Push-Location $probeRepo
    try {
        cargo rustc --release -p bars-app --example docking_probe -- -C link-arg=/MANIFEST:EMBED -C "link-arg=/MANIFESTINPUT:$probeManifest"
        if ($LASTEXITCODE) { throw 'Не собран нативный стенд' }
    } finally { Pop-Location }
    $probeExe = Join-Path $probeTemp 'docking_probe.exe'
    Copy-Item -LiteralPath (Join-Path $env:CARGO_TARGET_DIR 'release/examples/docking_probe.exe') -Destination $probeExe
    $probeRedist = Get-ChildItem (Join-Path $probeVs 'VC/Redist/MSVC') -Directory | Where-Object Name -Match '^\d' | Sort-Object Name -Descending | Select-Object -First 1
    Get-ChildItem (Join-Path $probeRedist.FullName 'x64/Microsoft.VC143.CRT') -Filter '*.dll' | Copy-Item -Destination $probeTemp
    $probeVulkan = Join-Path $env:CARGO_TARGET_DIR 'release/vulkan-1.dll'
    if (Test-Path $probeVulkan) { Copy-Item -LiteralPath $probeVulkan -Destination $probeTemp }
    $probeResult = Join-Path $probeTemp 'result.json'
    $probeProcess = Start-Process -FilePath $probeExe -ArgumentList ('"' + $probeResult + '" "' + $probeTemp + '"') -WindowStyle Hidden -PassThru
    if (-not $probeProcess.WaitForExit(55000)) {
        if ((Get-Process -Id $probeProcess.Id).Path -eq $probeExe) { Stop-Process -Id $probeProcess.Id }
        throw 'Таймаут нативного стенда; остановлен только его процесс'
    }
    Get-ChildItem $probeTemp -Filter '*.txt' | ForEach-Object { Get-Content $_.FullName }
    if (-not (Test-Path $probeResult)) { throw "Стенд не создал результат, exit=$($probeProcess.ExitCode)" }
    Copy-Item -LiteralPath $probeResult -Destination $probeOutput
    $probeReport = Get-Content -LiteralPath $probeResult -Raw | ConvertFrom-Json
    Get-Content -LiteralPath $probeResult
    if (-not $probeReport.success) { throw 'Нативные проверки не пройдены' }
} finally {
    $probeResolved = [IO.Path]::GetFullPath($probeTemp)
    $probeAllowed = [IO.Path]::GetFullPath([IO.Path]::GetTempPath())
    if ($probeResolved.StartsWith($probeAllowed) -and (Split-Path $probeResolved -Leaf).StartsWith('bars-docking-run-')) {
        # WebView2 отпускает профиль чуть позже основного процесса.
        for ($probeAttempt = 0; $probeAttempt -lt 10; $probeAttempt++) {
            try { Remove-Item -LiteralPath $probeResolved -Recurse -Force; break }
            catch { if ($probeAttempt -eq 9) { throw }; Start-Sleep -Milliseconds 300 }
        }
    }
}
