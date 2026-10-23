param(
    [Parameter(Mandatory)][string]$TemporaryRoot,
    [Parameter(Mandatory)][string]$BinaryDirectory,
    [string]$IndexBuildReadyFile,
    [switch]$DownloadFp16,
    [ValidateSet('fp32','fp16')][string[]]$Precisions=@('fp32','fp16'),
    [string]$CorpusBundle,
    [string]$EvidencePath='docs/chats/2026-10-23-indexing-memory',
    [ValidateRange(1,600)][int]$WaitSeconds=120
)
$ErrorActionPreference='Stop'
$temp=[IO.Path]::GetFullPath($TemporaryRoot)
$parent=[IO.Path]::GetFullPath([IO.Path]::GetTempPath()).TrimEnd('\')
if([IO.Path]::GetDirectoryName($temp) -ne $parent -or [IO.Path]::GetFileName($temp) -notmatch '^bars-memory-stage10-[a-f0-9]{32}$'){throw 'Unexpected owned temporary root'}
$evidence=(Resolve-Path -LiteralPath $EvidencePath).Path
$shared=Join-Path $env:USERPROFILE '.bars/models'
$models=Join-Path $temp 'models'
$links=@()
$phase='prepare'
$ok=$false
$failure=$null
$failedSeries=@()
function Wait-Headroom([int]$CommitGiB) {
    $deadline=[DateTime]::UtcNow.AddSeconds($WaitSeconds)
    $ready=0
    do {
        $m=Get-CimInstance Win32_PerfFormattedData_PerfOS_Memory
        $commit=[long]$m.CommitLimit-[long]$m.CommittedBytes
        $enough=$m.AvailableBytes -ge 4GB -and $commit -ge ($CommitGiB*1GB)
        @{utc=[DateTime]::UtcNow.ToString('o');phase=$phase;available_bytes=$m.AvailableBytes;free_commit_bytes=$commit;required_commit_gib=$CommitGiB;ready=$enough} | ConvertTo-Json -Compress | Add-Content -LiteralPath "$evidence/preflight.jsonl"
        if($enough){$ready++}else{$ready=0}
        if($ready -ge 3){return}
        if([DateTime]::UtcNow -ge $deadline){throw 'Insufficient natural headroom; no model started'}
        Start-Sleep -Seconds 2
    }while($true)
}
function Run-Owned([string]$Exe,[string[]]$Arguments,[string]$Name,[switch]$AllowFailure) {
    $quoted=$Arguments | ForEach-Object {'"'+$_+'"'}
    $child=Start-Process -FilePath $Exe -ArgumentList $quoted -WindowStyle Hidden -PassThru -RedirectStandardError "$temp/$Name.stderr.txt" -RedirectStandardOutput "$temp/$Name.stdout.txt"
    $started=[DateTime]::UtcNow
    try {
        while(!$child.HasExited){
            $gpu=@(Get-CimInstance Win32_PerfFormattedData_GPUPerformanceCounters_GPUProcessMemory -ErrorAction SilentlyContinue | Where-Object Name -Like "pid_$($child.Id)_*" | Select-Object Name,DedicatedUsage,SharedUsage,TotalCommitted)
            $foreign=@(Get-Process cargo,rustc,bench_worker_memory,bench_worker_index_memory -ErrorAction SilentlyContinue | Where-Object Id -NE $child.Id | Select-Object ProcessName,Id)
            @{utc=[DateTime]::UtcNow.ToString('o');gpu=$gpu;foreign=$foreign} | ConvertTo-Json -Depth 5 -Compress | Add-Content -LiteralPath "$temp/$Name.gpu.jsonl"
            if(([DateTime]::UtcNow-$started).TotalSeconds -gt 1500){throw 'Owned run timed out'}
            Start-Sleep -Milliseconds 500
            $child.Refresh()
        }
        $child.WaitForExit()
        @{pid=$child.Id;exit_code=$child.ExitCode;seconds=([DateTime]::UtcNow-$started).TotalSeconds} | ConvertTo-Json | Set-Content -LiteralPath "$temp/$Name.process.json"
        if($child.ExitCode -ne 0){
            if($AllowFailure){return $false}
            throw "Owned run $Name failed: $($child.ExitCode)"
        }
        if($AllowFailure){return $true}
    }finally{if(!$child.HasExited){$child.Kill();$child.WaitForExit()}}
}
function Save-TextGzip([string]$Source,[string]$Destination) {
    $reader=[IO.StreamReader]::new($Source)
    $stream=[IO.File]::Create($Destination)
    $gzip=[IO.Compression.GZipStream]::new($stream,[IO.Compression.CompressionLevel]::Optimal)
    $writer=[IO.StreamWriter]::new($gzip,[Text.UTF8Encoding]::new($false))
    try{
        if($reader.EndOfStream){$writer.WriteLine()}
        while($null -ne ($line=$reader.ReadLine())){
            foreach($entry in @(@($temp,'<stand>'),@($shared,'<models>'))){
                foreach($needle in @($entry[0].Replace('\','\\\\'),$entry[0].Replace('\','\\'),$entry[0],$entry[0].Replace('\','/'))){
                    $line=$line.Replace($needle,$entry[1])
                }
            }
            $writer.WriteLine($line)
        }
    }finally{$reader.Dispose();$writer.Dispose();$gzip.Dispose();$stream.Dispose()}
}
try {
    New-Item -ItemType Directory -Force $models | Out-Null
    foreach($name in @('models--BAAI--bge-m3','models--rozgo--bge-reranker-v2-m3','models--onnx-community--bge-reranker-v2-m3-ONNX')){
        $target=Join-Path $shared $name
        if(!(Test-Path -LiteralPath $target -PathType Container)){throw "Missing shared model $name"}
        $link=Join-Path $models $name
        New-Item -ItemType Junction -Path $link -Value $target | Out-Null
        $links+=@{path=$link;target=[IO.Path]::GetFullPath($target)}
    }
    # Rerank stamps contain relative paths; copied marker remains tied to unchanged files.
    New-Item -ItemType Directory "$models/bars-verified" | Out-Null
    Get-ChildItem -LiteralPath "$shared/bars-verified" -Filter 'rerank-fp16-*.json' | Copy-Item -Destination "$models/bars-verified"
    $binaries=@('bench_inference.exe','bench_worker_memory.exe')
    $sources=@('Cargo.lock','crates/bars-core/examples/bench_memory.rs','crates/bars-worker/examples/bench_worker_memory.rs','crates/bars-worker/examples/bench_worker_index_memory.rs','crates/bars-worker/src/worker/models.rs','crates/bars-core/src/models/builtin.rs','crates/bars-core/src/models/builtin_rerank.rs','scripts/run-index-memory.ps1')
    @{utc=[DateTime]::UtcNow.ToString('o');binaries=@($binaries | ForEach-Object {@{file=$_;sha256=(Get-FileHash -LiteralPath (Join-Path $BinaryDirectory $_)).Hash}});sources=@($sources | ForEach-Object {@{file=$_;sha256=(Get-FileHash -LiteralPath $_).Hash}});bars=@(Get-Process bars -ErrorAction SilentlyContinue | Select-Object Id,StartTime);timing_valid=$false} | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath "$evidence/provenance.json"
    if($IndexBuildReadyFile){
        $phase='wait-index-build'
        $deadline=[DateTime]::UtcNow.AddMinutes(20)
        while(!(Test-Path -LiteralPath $IndexBuildReadyFile)){
            if([DateTime]::UtcNow -ge $deadline){throw 'Index benchmark build not ready'}
            Start-Sleep -Seconds 2
        }
    }
    @{file='bench_worker_index_memory.exe';sha256=(Get-FileHash -LiteralPath (Join-Path $BinaryDirectory 'bench_worker_index_memory.exe')).Hash} | ConvertTo-Json | Set-Content -LiteralPath "$evidence/index-binary.json"
    # Freeze once: both precisions must see the same bytes despite parallel edits.
    $snapshot=Join-Path $temp 'corpus'
    $manifest=@()
    if($CorpusBundle){
        & node scripts/prepare-index-memory-corpus.mjs unpack $CorpusBundle $snapshot "$evidence/corpus.json"
        if($LASTEXITCODE -ne 0){throw 'Frozen corpus validation failed'}
    }else{foreach($file in Get-ChildItem docs -Recurse -File -Filter '*.md'){
        $relative=[IO.Path]::GetRelativePath((Join-Path (Get-Location).Path 'docs'),$file.FullName)
        $destination=Join-Path $snapshot $relative
        New-Item -ItemType Directory ([IO.Path]::GetDirectoryName($destination)) -Force | Out-Null
        Copy-Item -LiteralPath $file.FullName -Destination $destination
        $manifest+=@{file=$relative;sha256=(Get-FileHash -LiteralPath $destination).Hash}
    }
    $manifest | ConvertTo-Json | Set-Content -LiteralPath "$evidence/corpus.json"
    }
    foreach($precision in $Precisions){
        if($precision -eq 'fp16'){
            if($DownloadFp16){
                $phase='download-fp16'
                Wait-Headroom 2
                & node scripts/download-memory-fp16.mjs $temp *> "$temp/download.log"
                if($LASTEXITCODE -ne 0){throw 'Pinned FP16 artifact download failed'}
            }
            $phase='validate-fp16-artifact'
            Wait-Headroom 2
            Run-Owned (Join-Path $BinaryDirectory 'bench_inference.exe') @('download-embed-fp16',$models,"$temp/artifact.jsonl") 'artifact'
            Copy-Item -LiteralPath docs/knowledge/memory-acceptance-2026-10-20/fixture.json -Destination "$temp/fixture.json"
            for($n=1;$n -le 2;$n++){
                $phase="pair-$n"
                Wait-Headroom 8
                $passed=Run-Owned (Join-Path $BinaryDirectory 'bench_worker_memory.exe') @('bounded','auto',$models,"$temp/fixture.json","$temp/pair-$n.jsonl",'--fp16-embed') "pair-$n" -AllowFailure
                if(!$passed){$failedSeries+=$phase;break}
            }
        }
        $project=Join-Path $temp "project-$precision"
        New-Item -ItemType Directory "$project/docs" -Force | Out-Null
        foreach($file in Get-ChildItem -LiteralPath $snapshot -Recurse -File -Filter '*.md'){
            $relative=[IO.Path]::GetRelativePath($snapshot,$file.FullName)
            $destination=Join-Path "$project/docs" $relative
            New-Item -ItemType Directory ([IO.Path]::GetDirectoryName($destination)) -Force | Out-Null
            Copy-Item -LiteralPath $file.FullName -Destination $destination
        }
        foreach($rerank in @('off','on')){foreach($clients in @(1,2,4)){
            $phase="$precision-$rerank-$clients"
            Wait-Headroom 8
            $passed=Run-Owned (Join-Path $BinaryDirectory 'bench_worker_index_memory.exe') @($precision,$rerank,"$clients",$models,$project,"$temp/$phase") $phase -AllowFailure
            if(!$passed){$failedSeries+=$phase}
        }}
    }
    $ok=$failedSeries.Count -eq 0
    if(!$ok){$failure='Failed series: '+($failedSeries -join ', ')}
}catch{$failure=$_.Exception.Message;Write-Output "Stopped at ${phase}: $failure"}
finally {
    try {
        foreach($file in Get-ChildItem -LiteralPath $temp -File -Recurse | Where-Object { $_.FullName -notlike "$models*" -and $_.FullName -notlike "$temp/project-*" -and $_.Extension -in @('.json','.jsonl','.txt') }){
            $name=[IO.Path]::GetRelativePath($temp,$file.FullName).Replace('\','-')+'.gz'
            Save-TextGzip $file.FullName (Join-Path $evidence $name)
        }
    }finally {
        foreach($entry in $links){
            $item=Get-Item -LiteralPath $entry.path -Force
            if($item.LinkType -ne 'Junction' -or [IO.Path]::GetFullPath([string]@($item.Target)[0]) -ne $entry.target){throw 'Unexpected shared model link; cleanup refused'}
            [IO.Directory]::Delete($entry.path)
        }
        # Root was checked as a direct UUID child of TEMP; shared junctions removed first.
        Remove-Item -LiteralPath $temp -Recurse -Force
        @{ok=$ok;phase=$phase;error=$failure;temporary_root_removed=!(Test-Path -LiteralPath $temp);bars=@(Get-Process bars -ErrorAction SilentlyContinue | Select-Object Id,StartTime)} | ConvertTo-Json -Depth 4 | Set-Content -LiteralPath "$evidence/pipeline.json"
    }
}
if(!$ok){exit 1}
