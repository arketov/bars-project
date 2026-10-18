# Passive Windows sampler; never starts/stops bars or changes system settings.
[CmdletBinding()]
param(
    [Parameter(Mandatory)][int]$RootProcessId,
    [Parameter(Mandatory)][string]$OutputPath,
    [ValidateRange(1, 3600)][int]$Samples = 10,
    [ValidateRange(100, 60000)][int]$IntervalMs = 1000,
    [string]$Phase = 'passive-existing-process'
)
$ErrorActionPreference = 'Stop'
if (Test-Path -LiteralPath $OutputPath) { throw 'Output exists; choose a new file.' }
$rootProcess = Get-Process -Id $RootProcessId
$rootStarted = $rootProcess.StartTime.ToUniversalTime().ToString('o')
$rootHash = (Get-FileHash -LiteralPath $rootProcess.Path -Algorithm SHA256).Hash
$rows = @()
for ($sample = 0; $sample -lt $Samples; $sample++) {
    $started = [DateTime]::UtcNow
    $currentRoot = Get-Process -Id $RootProcessId
    if ($currentRoot.StartTime.ToUniversalTime().ToString('o') -ne $rootStarted) {
        throw 'Root PID reused; measurement aborted.'
    }
    $inventory = @(Get-CimInstance Win32_Process -Property ProcessId,ParentProcessId,Name)
    $treeIds = [System.Collections.Generic.HashSet[int]]::new()
    [void]$treeIds.Add($RootProcessId)
    do {
        $added = $false
        foreach ($item in $inventory) {
            if ($treeIds.Contains([int]$item.ParentProcessId)) {
                if ($treeIds.Add([int]$item.ProcessId)) { $added = $true }
            }
        }
    } while ($added)
    $processRows = @()
    foreach ($treeId in $treeIds) {
        $process = Get-Process -Id $treeId -ErrorAction SilentlyContinue
        if ($null -eq $process) { continue }
        $process.Refresh()
        $processRows += [ordered]@{
            pid = $treeId
            name = $process.ProcessName
            working_set_bytes = $process.WorkingSet64
            private_bytes = $process.PrivateMemorySize64
            peak_working_set_bytes = $process.PeakWorkingSet64
        }
    }
    $memory = Get-CimInstance Win32_PerfFormattedData_PerfOS_Memory
    $os = Get-CimInstance Win32_OperatingSystem
    $pagefiles = @(Get-CimInstance Win32_PageFileUsage | ForEach-Object {
        [ordered]@{ allocated_mib = $_.AllocatedBaseSize; used_mib = $_.CurrentUsage; peak_used_mib = $_.PeakUsage }
    })
    $rows += [ordered]@{
        utc = $started.ToString('o')
        collection_ms = ([DateTime]::UtcNow - $started).TotalMilliseconds
        phase = $Phase
        processes = $processRows
        system = [ordered]@{
            physical_total_bytes = [long]$os.TotalVisibleMemorySize * 1KB
            available_bytes = $memory.AvailableBytes
            committed_bytes = $memory.CommittedBytes
            commit_limit_bytes = $memory.CommitLimit
            pages_input_per_sec = $memory.PagesInputPersec
            pages_output_per_sec = $memory.PagesOutputPersec
            page_reads_per_sec = $memory.PageReadsPersec
            pagefiles = $pagefiles
        }
    }
    if ($sample + 1 -lt $Samples) { Start-Sleep -Milliseconds $IntervalMs }
}
[ordered]@{
    schema = 1
    root_pid = $RootProcessId
    root_started_utc = $rootStarted
    root_exe_sha256 = $rootHash
    root_file_version = $rootProcess.FileVersion
    requested_interval_ms = $IntervalMs
    note = 'Sequential non-atomic samples; interval excludes collection time. WS sum includes shared pages. Peaks predate sampling. Paging counters are system-wide.'
    rows = $rows
} | ConvertTo-Json -Depth 9 | Set-Content -LiteralPath $OutputPath -Encoding utf8
Write-Output "Saved $($rows.Count) samples to $OutputPath"
