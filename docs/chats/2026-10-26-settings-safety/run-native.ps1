param([string]$Directory = '.bars/task-315/native-final')
$ErrorActionPreference = 'Stop'
$probeRun = [IO.Path]::GetFullPath((Join-Path $PWD $Directory))
New-Item -ItemType Directory -Path $probeRun -Force | Out-Null
Copy-Item -LiteralPath C:/bt/bars/debug/examples/workspace_probe.exe -Destination (Join-Path $probeRun 'workspace_probe.exe')
$probeVswhere = Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio/Installer/vswhere.exe'
$probeVs = & $probeVswhere -latest -products '*' -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
$probeRedist = Get-ChildItem (Join-Path $probeVs 'VC/Redist/MSVC') -Directory | Where-Object Name -Match '^\d' | Sort-Object Name -Descending | Select-Object -First 1
Get-ChildItem (Join-Path $probeRedist.FullName 'x64') -Directory -Filter 'Microsoft.VC*.CRT' | Get-ChildItem -Filter '*.dll' | Copy-Item -Destination $probeRun
$env:BARS_USER_HOME = $probeRun
$probeScript = Join-Path $PWD 'crates/bars-app/examples/settings_safety_probe.js'
$probeProcess = Start-Process -FilePath (Join-Path $probeRun 'workspace_probe.exe') -ArgumentList @('"' + $probeRun + '"', '"' + $probeScript + '"') -WindowStyle Hidden -PassThru -Wait -RedirectStandardOutput (Join-Path $probeRun 'stdout.txt') -RedirectStandardError (Join-Path $probeRun 'stderr.txt')
$probeResult = Get-Content -LiteralPath (Join-Path $probeRun 'result.json') -Raw | ConvertFrom-Json
$probeResult | ConvertTo-Json -Depth 5
Copy-Item -LiteralPath (Join-Path $probeRun 'result.json') -Destination (Join-Path $PWD 'docs/chats/2026-10-26-settings-safety/native-result.json')
if ($probeProcess.ExitCode -ne 0 -or -not $probeResult.success) { exit 1 }
exit 0
