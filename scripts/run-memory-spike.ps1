[CmdletBinding()]
param(
    [Parameter(Mandatory)][string]$BenchPath,
    [Parameter(Mandatory)][string]$ModelsPath,
    [Parameter(Mandatory)][string]$EvidencePath,
    [ValidateSet('cpu','auto')][string[]]$Devices = @('cpu','auto'),
    [ValidateSet('rerank','embed','both','bounded')][string[]]$Modes = @('rerank','embed','both'),
    [ValidateRange(1,10)][int]$Repeat = 1,
    [ValidateRange(4,64)][int]$MinRamGiB = 4,
    [ValidateRange(6,128)][int]$MinCommitGiB = 8,
    [switch]$AllowDownload,
    [switch]$ExpectCpuFallback,
    [ValidatePattern('^[a-z0-9-]+$')][string]$Scenario = 'cached',
    [string]$BuildMetadataPath,
    [string]$SourcePath = 'crates/bars-core/examples/bench_memory.rs',
    [ValidateRange(30,7200)][int]$MaxRunSeconds = 1800,
    [ValidateRange(0,3600)][int]$WaitSeconds = 120
)
$ErrorActionPreference = 'Stop'
$bench = (Resolve-Path -LiteralPath $BenchPath).Path
$evidence = (Resolve-Path -LiteralPath $EvidencePath).Path
$fixture = Join-Path $evidence 'fixture.json'
$invocation = [DateTime]::UtcNow.ToString('yyyyMMddTHHmmssfffZ')
$preflightLog = Join-Path $evidence "preflight-$invocation.jsonl"
$sources = @('Cargo.lock','crates/bars-core/examples/bench_memory.rs','crates/bars-core/src/models/builtin.rs','crates/bars-core/src/models/execution.rs','crates/bars-core/src/models/builtin_rerank.rs','crates/bars-core/src/models/rerank_artifact.rs','crates/bars-worker/src/worker/models.rs','crates/bars-worker/src/worker/models/resident.rs','crates/bars-worker/src/worker/model_lifecycle.rs','crates/bars-worker/src/worker/mod.rs') | ForEach-Object {
    [ordered]@{file=$_;sha256=(Get-FileHash -LiteralPath $_).Hash}
}
$benchHash=(Get-FileHash -LiteralPath $bench).Hash
$sourceHash=(Get-FileHash -LiteralPath $SourcePath).Hash
$buildSources=$sources
if ($BuildMetadataPath) {
    $build=Get-Content -LiteralPath $BuildMetadataPath -Raw | ConvertFrom-Json
    if ($build.bench_sha256 -ne $benchHash) { throw 'Binary differs from supplied build metadata' }
    $sourceHash=$build.source_sha256
    $buildSources=$build.sources
}
[ordered]@{utc=[DateTime]::UtcNow.ToString('o');scenario=$Scenario;bench_sha256=$benchHash;source_sha256=$sourceHash;fixture_sha256=(Get-FileHash -LiteralPath $fixture).Hash;sources=$buildSources;current_sources=$sources;devices=$Devices;modes=$Modes;repeats=$Repeat;download_allowed=[bool]$AllowDownload} |
    ConvertTo-Json -Depth 4 | Set-Content -LiteralPath (Join-Path $evidence "run-$invocation.json") -Encoding utf8
foreach ($device in $Devices) {
    foreach ($mode in $Modes) {
        for ($round = 1; $round -le $Repeat; $round++) {
            $deadline = [DateTime]::UtcNow.AddSeconds($WaitSeconds)
            $readyCount = 0
            do {
                $memory = Get-CimInstance Win32_PerfFormattedData_PerfOS_Memory
                $freeCommit = [long]$memory.CommitLimit - [long]$memory.CommittedBytes
                $foreign = @(Get-Process cargo,rustc,bench_inference,bench_worker_inference,bench_memory,bench_worker_memory -ErrorAction SilentlyContinue | Select-Object ProcessName,Id)
                $ready = [long]$memory.AvailableBytes -ge ($MinRamGiB * 1GB) -and $freeCommit -ge ($MinCommitGiB * 1GB) -and $foreign.Count -eq 0
                [ordered]@{utc=[DateTime]::UtcNow.ToString('o');mode=$mode;device=$device;round=$round;available_bytes=$memory.AvailableBytes;free_commit_bytes=$freeCommit;min_ram_gib=$MinRamGiB;min_commit_gib=$MinCommitGiB;foreign=$foreign;ready=$ready} |
                    ConvertTo-Json -Compress | Add-Content -LiteralPath $preflightLog -Encoding utf8
                if ($ready) { $readyCount++ } else { $readyCount = 0 }
                if ($readyCount -ge 3) { break }
                if ([DateTime]::UtcNow -ge $deadline) {
                    throw "No quiet window with sufficient headroom for $mode/$device; no new model started."
                }
                Start-Sleep -Seconds 2
            } while ($true)
            $stem = "$invocation-$Scenario-$device-$mode-$round"
            $output = Join-Path $evidence "$stem.jsonl"
            $stderr = Join-Path $evidence "$stem.stderr.txt"
            Write-Output "Starting $stem"
            # All paths are literal arguments; no shell interpretation. Hidden owned child.
            $arguments = @($mode,$device,$ModelsPath,$fixture,$output) | ForEach-Object { '"' + $_ + '"' }
            if ($AllowDownload) { $arguments += '--allow-download' }
            if ($ExpectCpuFallback) { $arguments += '--expect-cpu-fallback' }
            $child = Start-Process -FilePath $bench -ArgumentList $arguments -WindowStyle Hidden -RedirectStandardError $stderr -PassThru
            $runStart = [DateTime]::UtcNow
            $timedOut = $false
            $gpuLog = Join-Path $evidence "$stem.gpu.jsonl"
            do {
                $gpu = @(Get-CimInstance Win32_PerfFormattedData_GPUPerformanceCounters_GPUProcessMemory -ErrorAction SilentlyContinue |
                    Where-Object Name -Like "pid_$($child.Id)_*" | Select-Object Name,DedicatedUsage,SharedUsage,TotalCommitted)
                $foreign = @(Get-Process cargo,rustc,bench_inference,bench_worker_inference,bench_memory,bench_worker_memory -ErrorAction SilentlyContinue |
                    Where-Object Id -NE $child.Id | Select-Object ProcessName,Id)
                [ordered]@{utc=[DateTime]::UtcNow.ToString('o');pid=$child.Id;gpu=$gpu;foreign=$foreign} |
                    ConvertTo-Json -Depth 5 -Compress | Add-Content -LiteralPath $gpuLog -Encoding utf8
                if (!$child.HasExited -and ([DateTime]::UtcNow - $runStart).TotalSeconds -gt $MaxRunSeconds) {
                    $timedOut = $true
                    $child.Kill()
                }
                if (!$child.HasExited) { Start-Sleep -Milliseconds 500 }
                $child.Refresh()
            } while (!$child.HasExited)
            $child.WaitForExit()
            [ordered]@{utc=[DateTime]::UtcNow.ToString('o');pid=$child.Id;exit_code=$child.ExitCode;timed_out=$timedOut} |
                ConvertTo-Json | Set-Content -LiteralPath (Join-Path $evidence "$stem.process.json") -Encoding utf8
            $stderrText = [IO.File]::ReadAllText($stderr)
            $stderrText = $stderrText.Replace($ModelsPath, '<models>').Replace($ModelsPath.Replace('\','/'), '<models>')
            [IO.File]::WriteAllText($stderr, $stderrText)
            if (Test-Path -LiteralPath $output) {
                $encodedModels = ConvertTo-Json -InputObject $ModelsPath -Compress
                $encodedModels = $encodedModels.Substring(1, $encodedModels.Length - 2)
                $rowsText = [IO.File]::ReadAllText($output).Replace($encodedModels, '<models>').Replace($ModelsPath.Replace('\','/'), '<models>')
                [IO.File]::WriteAllText($output, $rowsText)
            }
            if ($child.ExitCode -ne 0) { throw "Memory spike exited $($child.ExitCode); see $output and $stderr" }
            Write-Output "Completed $stem"
        }
    }
}
