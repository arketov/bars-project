# Сначала cargo build -p bars-worker --features test-support --example client_refresh_session.
param([string]$EvidenceDir = (Join-Path $PSScriptRoot '../target/stage16/delivery'))
$ErrorActionPreference = 'Stop'
$repo16 = Split-Path $PSScriptRoot
$EvidenceDir = [IO.Path]::GetFullPath($EvidenceDir)
$sessionHome16 = Join-Path $EvidenceDir 'home'
$project16 = Join-Path $EvidenceDir 'project'
$worker16 = Join-Path $repo16 'target/debug/examples/client_refresh_session.exe'
$cli16 = Join-Path $repo16 'target/debug/bars-cli.exe'
New-Item -ItemType Directory -Force -Path $sessionHome16,(Join-Path $project16 '.bars'),(Join-Path $project16 'manual/existing-notes') | Out-Null
[IO.File]::WriteAllText((Join-Path $project16 '.bars/config.toml'), "docs_root = 'manual'`n")
$untouched16 = Join-Path $project16 'manual/existing-notes/info.md'
$source16 = "---`ntitle: Existing project note`n---`n`n# Existing project note`n`nUser layout stays here.`n"
[IO.File]::WriteAllText($untouched16, $source16)
$expected16 = [IO.File]::ReadAllText((Join-Path $repo16 'crates/bars-core/assets/BARS_INSTRUCTION.md'))
function New-Info16([string]$exe16, [string[]]$arguments16) {
    $info16 = [Diagnostics.ProcessStartInfo]::new($exe16)
    $info16.UseShellExecute = $false
    $info16.CreateNoWindow = $true
    $info16.WorkingDirectory = $project16
    $info16.RedirectStandardOutput = $true
    $info16.RedirectStandardError = $true
    $info16.Environment['BARS_USER_HOME'] = $sessionHome16
    $info16.Environment['CODEX_HOME'] = Join-Path $sessionHome16 '.codex'
    $info16.Environment['BARS_TEST_FAKE_MODELS'] = '1'
    foreach ($arg16 in $arguments16) { $info16.ArgumentList.Add($arg16) }
    return $info16
}
function Cli16([string[]]$arguments16) {
    $child16 = [Diagnostics.Process]::Start((New-Info16 $cli16 $arguments16))
    try {
        $out16 = $child16.StandardOutput.ReadToEndAsync()
        $err16 = $child16.StandardError.ReadToEndAsync()
        if (-not $child16.WaitForExit(15000)) { $child16.Kill($true); throw 'CLI timeout' }
        if ($child16.ExitCode) { throw $err16.GetAwaiter().GetResult() }
        return $out16.GetAwaiter().GetResult()
    } finally { $child16.Dispose() }
}
function Wait16([scriptblock]$condition16) {
    $deadline16 = [DateTime]::UtcNow.AddSeconds(20)
    while (-not (& $condition16)) {
        if ([DateTime]::UtcNow -gt $deadline16) { throw 'Delivery timeout' }
        Start-Sleep -Milliseconds 100
    }
}
$process16 = [Diagnostics.Process]::Start((New-Info16 $worker16 @()))
$prefix16 = [Collections.Generic.List[string]]::new()
$log16 = $null
try {
    # ready в stderr выводится после bind; CLI не должен запускать запасной воркер.
    $readyDeadline16 = [DateTime]::UtcNow.AddSeconds(20)
    do {
        $line16 = $process16.StandardError.ReadLineAsync()
        if (-not $line16.Wait(10000) -or $null -eq $line16.Result -or [DateTime]::UtcNow -gt $readyDeadline16) { throw 'Worker did not become ready' }
        $prefix16.Add($line16.Result)
    } while ($line16.Result -ne 'session: ready')
    $log16 = $process16.StandardError.ReadToEndAsync()
    Cli16 @('register',$project16,'--json') | Set-Content (Join-Path $EvidenceDir 'register.json')
    $instruction16 = Join-Path $project16 'manual/BARS_INSTRUCTION.md'
    Wait16 { (Test-Path $instruction16) -and [IO.File]::ReadAllText($instruction16) -ceq $expected16 }
    Wait16 { $state16 = (Cli16 @('status','--project',$project16,'--json') | ConvertFrom-Json); $state16.state -eq 'ready' }
    $beforeUpdate16 = $state16.updated_at
    $time16 = (Get-Item $instruction16).LastWriteTimeUtc.Ticks
    $null = Cli16 @('reindex','--project',$project16,'--json')
    Wait16 { $state16 = (Cli16 @('status','--project',$project16,'--json') | ConvertFrom-Json); $state16.state -eq 'ready' -and $state16.updated_at -ne $beforeUpdate16 }
    if ((Get-Item $instruction16).LastWriteTimeUtc.Ticks -ne $time16) { throw 'No-op rewrote instruction' }
    [IO.File]::WriteAllText($instruction16, 'old instruction')
    $null = Cli16 @('reindex','--project',$project16,'--json')
    Wait16 { [IO.File]::ReadAllText($instruction16) -ceq $expected16 }
    $projects16 = Cli16 @('projects','--json')
    $projects16 | Set-Content (Join-Path $EvidenceDir 'projects.json')
    if ($projects16 -notmatch 'existing-notes') { throw 'Custom root was not indexed' }
    if ([IO.File]::ReadAllText($untouched16) -cne $source16 -or (Test-Path (Join-Path $project16 'docs')) -or (Test-Path (Join-Path $project16 'manual/backlog'))) { throw 'Layout changed' }
    @{ passed=$true; custom_root='manual'; fake_models=$true; instruction_repaired=$true; noop_mtime=$true; layout_preserved=$true } | ConvertTo-Json | Tee-Object -FilePath (Join-Path $EvidenceDir 'summary.json')
} finally {
    [IO.File]::WriteAllText((Join-Path $sessionHome16 'stop'), '')
    if (-not $process16.WaitForExit(10000)) { $process16.Kill($true); $process16.WaitForExit() }
    $tail16 = if ($null -ne $log16) { $log16.GetAwaiter().GetResult() } else { '' }
    ($prefix16 -join "`n") + "`n" + $tail16 | Set-Content (Join-Path $EvidenceDir 'worker.stderr.txt')
    $process16.Dispose()
}
