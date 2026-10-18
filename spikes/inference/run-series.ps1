param(
    [Parameter(Mandatory)][string]$Executable,
    [Parameter(Mandatory)][string]$Data,
    [Parameter(Mandatory)][string]$Models,
    [Parameter(Mandatory)][string[]]$Configs,
    [Parameter(Mandatory)][string]$OutputRoot,
    [ValidateSet('run','full','quality','embed','scale','index','worker','profile','binding')][string]$Mode='run',
    [string]$Snapshot,
    [string]$ModelFile,
    [string]$QualityReference,
    [ValidateRange(0,32)][int]$MinimumCommitGiB=0,
    [ValidateRange(20,3600)][int]$PreflightTimeoutSeconds=120,
    [switch]$QualityOnly
)
$ErrorActionPreference='Stop'
if($Mode -eq 'quality' -and !$QualityOnly){throw 'staged quality requires QualityOnly; timings are not product latency'}
function Get-BarLoad([hashtable]$Previous){
    $apps=@(Get-Process bars -ErrorAction SilentlyContinue)
    if(!$apps.Count){return}
    $engines=@(Get-CimInstance Win32_PerfFormattedData_GPUPerformanceCounters_GPUEngine -ErrorAction SilentlyContinue)
    foreach($app in $apps){
        $cpu=$app.TotalProcessorTime.TotalSeconds
        $delta=if($Previous.ContainsKey($app.Id)){[math]::Max(0,$cpu-$Previous[$app.Id])}else{0}
        $Previous[$app.Id]=$cpu
        $gpu=($engines | Where-Object {$_.Name.StartsWith("pid_$($app.Id)_")} | Measure-Object UtilizationPercentage -Sum).Sum
        [pscustomobject]@{pid=$app.Id;cpu_delta=$delta;gpu_utilization=[double]$gpu}
    }
}
$root=(Get-Location).Path
$Executable=(Resolve-Path $Executable).Path
$Data=(Resolve-Path $Data).Path
$Models=(Resolve-Path $Models).Path
$minimumCommit=if($Mode -eq 'run'){9GB}else{12GB}
if($MinimumCommitGiB){
    $floor=if($Mode -eq 'quality'){6}else{9}
    if($MinimumCommitGiB -lt $floor){throw "MinimumCommitGiB must retain at least $floor GiB"}
    $minimumCommit=$MinimumCommitGiB*1GB
}
$nvidiaCommand=Get-Command nvidia-smi -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force $OutputRoot | Out-Null
$OutputRoot=(Resolve-Path $OutputRoot).Path
foreach($configPath in $Configs){
    $configPath=(Resolve-Path $configPath).Path
    $cfg=Get-Content -LiteralPath $configPath -Raw | ConvertFrom-Json
    $out=Join-Path $OutputRoot $cfg.name
    if(Test-Path -LiteralPath $out){Write-Output "Already exists: $out";continue}
    $quiet=0
    $barCpu=@{}
    $preflight=[Diagnostics.Stopwatch]::StartNew()
    while($quiet -lt 4){
        if($preflight.Elapsed.TotalSeconds -ge $PreflightTimeoutSeconds){throw "Preflight timeout: insufficient quiet RAM/commit window; no model process started"}
        $memory=Get-CimInstance Win32_PerfFormattedData_PerfOS_Memory
        $free=[long]$memory.AvailableMBytes*1MB
        $headroom=[long]$memory.CommitLimit-[long]$memory.CommittedBytes
        $minRam=if($Mode -eq 'quality'){4GB}else{8GB}
        if($free -lt $minRam -or $headroom -lt $minimumCommit){$quiet=0;Start-Sleep -Seconds 5;continue}
        $other=@(Get-Process bench_memory,bench_quality,bench_inference,bench_worker_inference,rustc,cargo -ErrorAction SilentlyContinue)
        $bar=@(Get-BarLoad $barCpu)
        $barActive=@($bar | Where-Object {$_.cpu_delta -gt .25 -or $_.gpu_utilization -gt 1}).Count -gt 0
        if(($other.Count -or $barActive) -and !$QualityOnly){$quiet=0}else{$quiet++}
        Start-Sleep -Seconds 5
    }
    $start=[DateTime]::UtcNow.ToString('o')
    $arguments=@($Mode,$Data,$Models,$configPath,$out)
    if($Mode -in @('profile','binding')){
        if(!$ModelFile){throw 'profile/binding mode requires ModelFile'}
        $arguments=@($Mode,$Data,$Models,$configPath,(Resolve-Path $ModelFile).Path,$out)
    }
    if($Mode -eq 'worker'){
        if(!$Snapshot){throw 'worker mode requires Snapshot'}
        $arguments=@((Resolve-Path $Snapshot).Path,$Data,$Models,$configPath,$out)
    }
    $process=Start-Process -FilePath $Executable -ArgumentList $arguments -WindowStyle Hidden -PassThru -RedirectStandardOutput "$out.stdout.log" -RedirectStandardError "$out.stderr.log"
    $samples=@()
    $memoryAbort=$false
    $qualityRejected=$false
    $observedRowsSize=-1L
    try {
    do{
        $process.Refresh()
        $memory=Get-CimInstance Win32_PerfFormattedData_PerfOS_Memory
        $free=[long]$memory.AvailableMBytes*1MB
        $headroom=[long]$memory.CommitLimit-[long]$memory.CommittedBytes
        $abortRam=if($Mode -eq 'quality'){512MB}else{2GB}
        $abortCommit=if($Mode -eq 'quality'){1GB}else{4GB}
        if($free -lt $abortRam -or $headroom -lt $abortCommit){
            $memoryAbort=$true
            if(!$process.HasExited){$process.Kill()}
        }
        $other=@()
        if(!$memoryAbort){$other=@(Get-Process bench_memory,bench_quality,bench_inference,bench_worker_inference,rustc,cargo -ErrorAction SilentlyContinue | Where-Object Id -ne $process.Id | Select-Object -ExpandProperty ProcessName)}
        $bar=@(Get-BarLoad $barCpu)
        if(@($bar | Where-Object {$_.cpu_delta -gt .1 -or $_.gpu_utilization -gt 1}).Count){$other+=@('bars-active')}
        $gpu=@(Get-CimInstance Win32_PerfFormattedData_GPUPerformanceCounters_GPUProcessMemory -ErrorAction SilentlyContinue | Where-Object {$_.Name.StartsWith("pid_$($process.Id)_")} | Select-Object Name,DedicatedUsage,SharedUsage,TotalCommitted)
        $adapter=$null
        if($nvidiaCommand -and $samples.Count%5 -eq 0){$adapter=(& $nvidiaCommand.Source --query-gpu=temperature.gpu,clocks.current.graphics,clocks.current.memory,pstate,power.draw,memory.used --format=csv,noheader,nounits 2>$null) -join ';'}
        $samples+=@{at=[DateTime]::UtcNow.ToString('o');private_bytes=$process.PrivateMemorySize64;working_set=$process.WorkingSet64;peak_working_set=$process.PeakWorkingSet64;cpu_seconds=$process.TotalProcessorTime.TotalSeconds;available_ram=$free;commit_headroom=$headroom;other=$other;bars_load=$bar;gpu=$gpu;adapter_csv=$adapter}
        if($QualityReference -and !$memoryAbort -and (Test-Path "$out/rows.jsonl")){
            if(!$QualityOnly){throw 'quality guard requires explicit QualityOnly; do not mix with clean latency selection'}
            $rowsSize=(Get-Item -LiteralPath "$out/rows.jsonl").Length
            if($rowsSize -ne $observedRowsSize){
                $observedRowsSize=$rowsSize
                node scripts/guard-inference-quality.mjs "$Data/inputs.json" $QualityReference "$out/rows.jsonl" "$out/rejected.json"
                if($LASTEXITCODE -eq 2){$qualityRejected=$true;if(!$process.HasExited){$process.Kill()}}
                elseif($LASTEXITCODE){throw 'Quality guard failed'}
            }
        }
        Start-Sleep -Seconds 1
    }while(!$process.HasExited)
    $process.WaitForExit()
    } finally {
        $process.Refresh()
        if(!$process.HasExited){$process.Kill();$process.WaitForExit()}
    }
    @{name=$cfg.name;pid=$process.Id;started=$start;minimum_commit=$minimumCommit;exit_code=$process.ExitCode;samples=$samples;quality_rejected=$qualityRejected;quality_only=[bool]$QualityOnly;timing_contaminated=$QualityOnly -or $memoryAbort -or @($samples | Where-Object {$_.other.Count -gt 0}).Count -gt 0} | ConvertTo-Json -Depth 8 | Set-Content "$out.monitor.json"
    if($memoryAbort){
        New-Item -ItemType Directory -Force $out | Out-Null
        @{status='invalidated';reason='RAM/commit pressure';at=[DateTime]::UtcNow.ToString('o')} | ConvertTo-Json | Set-Content "$out/invalidated.json"
        throw "Invalidated under memory pressure $($cfg.name); remaining series not started"
    }
    if($process.ExitCode -ne 0){
        if(Test-Path (Join-Path $out 'rejected.json')){Write-Output "Rejected experiment $($cfg.name)";continue}
        Get-Content "$out.stderr.log" -Tail 10;throw "Experiment failed: $($cfg.name)"
    }
    Write-Output "Complete $($cfg.name)"
}
