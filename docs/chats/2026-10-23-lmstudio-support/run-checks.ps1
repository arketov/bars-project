param([switch]$NativeOnly)
$ErrorActionPreference = 'Stop'
$env:PATH = "$env:USERPROFILE/.cargo/bin;$env:PATH"
$env:CMAKE_BUILD_PARALLEL_LEVEL = '1'
$env:CMAKE_GENERATOR = 'Ninja'
$env:VULKAN_SDK = (Get-ChildItem C:/tools/VulkanSDK -Directory | Sort-Object Name -Descending | Select-Object -First 1).FullName
$vswhere = Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio/Installer/vswhere.exe'
$vsPath = & $vswhere -latest -products '*' -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
& (Join-Path $vsPath 'Common7/Tools/Launch-VsDevShell.ps1') -Arch amd64 -HostArch amd64 -SkipAutomaticLocation
$env:LIB = "$env:VULKAN_SDK/Lib;$env:LIB"
$env:PATH = "$vsPath/Common7/IDE/CommonExtensions/Microsoft/CMake/Ninja;$env:PATH"
$evidence = Join-Path (Get-Location) 'docs/chats/2026-10-23-lmstudio-support'
if (-not $NativeOnly) {
    cargo test --release --target-dir C:/bt/bars -p bars-worker --lib handler::tests::external_models > "$evidence/worker-http.log" 2>&1
    if ($LASTEXITCODE) { throw 'worker HTTP tests failed' }
}
$manifest = Join-Path (Get-Location) 'crates/bars-app/examples/workspace_probe.manifest'
cargo rustc --release --target-dir C:/bt/bars -p bars-app --example lmstudio_probe -- -C link-arg=/MANIFEST:EMBED -C "link-arg=/MANIFESTINPUT:$manifest" > "$evidence/native-build.log" 2>&1
if ($LASTEXITCODE) { throw 'native probe build failed' }
$runtime = Join-Path ([IO.Path]::GetTempPath()) ('bars-lmstudio-window-' + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $runtime | Out-Null
try {
    $exe = Join-Path $runtime 'lmstudio_probe.exe'
    Copy-Item -LiteralPath C:/bt/bars/release/examples/lmstudio_probe.exe -Destination $exe
    Get-ChildItem C:/bt/bars/release -Filter '*.dll' | Copy-Item -Destination $runtime
    $redist = Get-ChildItem (Join-Path $vsPath 'VC/Redist/MSVC') -Directory | Where-Object Name -Match '^\d' | Sort-Object Name -Descending | Select-Object -First 1
    Get-ChildItem (Join-Path $redist.FullName 'x64') -Directory -Filter 'Microsoft.VC*.CRT' | Get-ChildItem -Filter '*.dll' | Copy-Item -Destination $runtime
    $env:BARS_USER_HOME = $runtime
    $probe = Start-Process -FilePath $exe -ArgumentList ('"' + $runtime + '"') -WindowStyle Hidden -PassThru -RedirectStandardOutput "$evidence/native-run.log" -RedirectStandardError "$evidence/native-error.log"
    if (-not $probe.WaitForExit(55000)) {
        $owned = Get-Process -Id $probe.Id -ErrorAction SilentlyContinue
        if ($owned -and $owned.Path -eq $exe) { Stop-Process -Id $owned.Id }
        throw 'native probe timeout; only owned process stopped'
    }
    if (-not (Test-Path (Join-Path $runtime 'result.json'))) { throw "native probe returned no result: $($probe.ExitCode)" }
    Copy-Item -LiteralPath (Join-Path $runtime 'result.json') -Destination (Join-Path $evidence 'result.json')
    $report = Get-Content "$evidence/result.json" -Raw | ConvertFrom-Json
    if (-not $report.success) { throw 'native probe checks failed' }
} finally {
    $resolved = [IO.Path]::GetFullPath($runtime)
    $temporaryRoot = [IO.Path]::GetFullPath([IO.Path]::GetTempPath())
    if ($resolved.StartsWith($temporaryRoot) -and (Split-Path $resolved -Leaf).StartsWith('bars-lmstudio-window-')) {
        # Только уборка: WebView2 отпускает кэш после выхода основного процесса.
        for ($cleanupAttempt = 0; $cleanupAttempt -lt 10; $cleanupAttempt++) {
            try { Remove-Item -LiteralPath $resolved -Recurse -Force; break }
            catch { if ($cleanupAttempt -eq 9) { throw }; Start-Sleep -Milliseconds 300 }
        }
    }
}
