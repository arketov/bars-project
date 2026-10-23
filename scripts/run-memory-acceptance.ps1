param([Parameter(Mandatory)][string]$TemporaryRoot)
$ErrorActionPreference='Stop'
$workspace=(Get-Location).Path
$evidence=Join-Path $workspace 'docs/knowledge/memory-acceptance-2026-10-20'
$temp=[IO.Path]::GetFullPath($TemporaryRoot)
$parent=[IO.Path]::GetFullPath([IO.Path]::GetTempPath()).TrimEnd('\')
if([IO.Path]::GetDirectoryName($temp) -ne $parent -or [IO.Path]::GetFileName($temp) -notmatch '^bars-memory-stage10-[a-f0-9]{32}$'){throw 'Unexpected temporary root'}
$models=Join-Path $temp 'models'
$shared=Join-Path $env:USERPROFILE '.bars/models'
$links=@()
$phase='initialization'
$ok=$false
$failure=$null
function Save-Gzip([string]$Source,[string]$Destination){
    $input=[IO.File]::OpenRead($Source);$output=[IO.File]::Create($Destination)
    $gzip=[IO.Compression.GZipStream]::new($output,[IO.Compression.CompressionLevel]::Optimal)
    try{$input.CopyTo($gzip)}finally{$gzip.Dispose();$output.Dispose();$input.Dispose()}
}
function Set-BuildEnvironment {
    $env:PATH='C:/BuildTools/VC/Tools/MSVC/14.44.35207/bin/Hostx64/x64;C:/Users/arketov/.rustup/toolchains/1.99.0-x86_64-pc-windows-msvc/bin;C:/Users/arketov/AppData/Local/bars-spikes/VulkanSDK/1.4.363.0/Bin;'+$env:PATH
    $env:LIB='C:/BuildTools/VC/Tools/MSVC/14.44.35207/lib/x64;C:/Program Files (x86)/Windows Kits/10/Lib/10.0.26100.0/ucrt/x64;C:/Program Files (x86)/Windows Kits/10/Lib/10.0.26100.0/um/x64'
    $env:INCLUDE='C:/BuildTools/VC/Tools/MSVC/14.44.35207/include;C:/Program Files (x86)/Windows Kits/10/Include/10.0.26100.0/ucrt;C:/Program Files (x86)/Windows Kits/10/Include/10.0.26100.0/shared;C:/Program Files (x86)/Windows Kits/10/Include/10.0.26100.0/um;C:/Program Files (x86)/Windows Kits/10/Include/10.0.26100.0/winrt'
    $env:VULKAN_SDK='C:/Users/arketov/AppData/Local/bars-spikes/VulkanSDK/1.4.363.0'
    $env:CARGO_TARGET_DIR='C:/bt/bars'
    $env:CMAKE_BUILD_PARALLEL_LEVEL='1'
}
try {
    New-Item -ItemType Directory -Force $models | Out-Null
    foreach($name in @('models--BAAI--bge-m3','models--rozgo--bge-reranker-v2-m3','models--onnx-community--bge-reranker-v2-m3-ONNX')){
        $target=Join-Path $shared $name
        if(!(Test-Path -LiteralPath $target -PathType Container)){throw "Missing shared model: $name"}
        $link=Join-Path $models $name
        $links+=@{path=$link;target=[IO.Path]::GetFullPath($target)}
        New-Item -ItemType Junction -Path $link -Value $target | Out-Null
    }
    $phase='build-core-benchmark'
    Write-Output $phase
    Set-BuildEnvironment
    & cargo build --release --offline -p bars-core --example bench_inference -j 1 *> "$evidence/rebuild-core.log"
    if($LASTEXITCODE -ne 0){throw 'Core benchmark build failed'}
    $bench='C:/bt/bars/release/examples/bench_inference.exe'
    $phase='core-example-tests'
    Write-Output $phase
    & cargo test --release --offline -p bars-core --example bench_inference -j 1 *> "$evidence/recovered-example-tests.log"
    if($LASTEXITCODE -ne 0){throw 'Core example tests failed'}
    $phase='download-one-fp16-artifact'
    Write-Output $phase
    & node scripts/download-memory-fp16.mjs $temp *> "$temp/range-download.log"
    if($LASTEXITCODE -ne 0){throw 'Bounded FP16 transfer failed'}
    $arguments=@('download-embed-fp16',$models,"$temp/artifact.jsonl") | ForEach-Object {'"'+$_+'"'}
    $download=Start-Process -FilePath $bench -ArgumentList $arguments -WindowStyle Hidden -PassThru -RedirectStandardError "$temp/download.log"
    try{
        if(!$download.WaitForExit(600000)){$download.Kill();$download.WaitForExit();throw 'Artifact transfer timed out'}
        if($download.ExitCode -ne 0){throw "Native artifact download failed: $($download.ExitCode)"}
    }finally{if(!$download.HasExited){$download.Kill();$download.WaitForExit()}}
    $phase='recover-frozen-inputs'
    & node scripts/prepare-memory-inputs.mjs $bench (Join-Path $env:LOCALAPPDATA 'bars/bench-quality') "$temp/data"
    if($LASTEXITCODE -ne 0){throw 'Frozen input validation failed'}
    @{name='core-fp16';device='directml';threads=16;batch=4;sorted=$true;max_length=8192;repeats=1;ids=@()} | ConvertTo-Json | Set-Content "$temp/embed.json"
    @{name='production';device='directml';threads=16;batch=4;sorted=$false;max_length=512;repeats=1;ids=@()} | ConvertTo-Json | Set-Content "$temp/rerank.json"
    foreach($corpus in @('bars','overdrive')){
        $phase="index-$corpus"
        Write-Output $phase
        & scripts/run-inference-spike.ps1 -Executable $bench -Data "$temp/data/$corpus" -Models $models -Configs "$temp/embed.json" -OutputRoot "$temp/index-$corpus" -Mode index-serial -QualityOnly -PreflightTimeoutSeconds 120
        $phase="quality-$corpus"
        Write-Output $phase
        & scripts/run-inference-spike.ps1 -Executable $bench -Data "$temp/index-$corpus/core-fp16/full-data" -Models $models -Configs "$temp/rerank.json" -OutputRoot "$temp/quality-$corpus" -Mode quality -MinimumCommitGiB 6 -QualityOnly -PreflightTimeoutSeconds 120
    }
    $phase='build-worker-benchmark'
    Write-Output $phase
    & cargo build --release --offline -p bars-worker --example bench_worker_memory -j 1 *> "$evidence/recovered-worker-build.log"
    if($LASTEXITCODE -ne 0){throw 'Worker benchmark build failed'}
    $phase='both-fp16-worker-pair'
    Write-Output $phase
    New-Item -ItemType Directory "$temp/pair" | Out-Null
    Copy-Item -LiteralPath "$evidence/fixture.json" -Destination "$temp/pair/fixture.json"
    & scripts/run-memory-spike.ps1 -BenchPath C:/bt/bars/release/examples/bench_worker_memory.exe -ModelsPath $models -EvidencePath "$temp/pair" -Devices auto -Modes bounded -Repeat 2 -Fp16Embed -MemoryOnly -Scenario both-fp16 -SourcePath crates/bars-worker/examples/bench_worker_memory.rs -WaitSeconds 120
    $phase='completed'
    $ok=$true
}catch{
    $failure=$_.Exception.Message
    Write-Output "Stopped at ${phase}: $failure"
}finally{
    try{
    # Only selected small outputs; weights, copied documents and worker profiles never become evidence.
    $folders=@('data/bars','data/overdrive','index-bars','index-bars/core-fp16','index-bars/core-fp16/full-data','quality-bars','quality-bars/production','index-overdrive','index-overdrive/core-fp16','index-overdrive/core-fp16/full-data','quality-overdrive','quality-overdrive/production','pair')
    foreach($relative in $folders){
        $folder=Join-Path $temp $relative
        if(!(Test-Path -LiteralPath $folder)){continue}
        foreach($file in Get-ChildItem -LiteralPath $folder -File){
            if($file.Extension -notin @('.json','.jsonl','.log','.sqlite','.txt')){continue}
            if($file.Extension -eq '.sqlite' -and $relative -notlike '*/full-data'){continue}
            $name=($relative.Replace('/','-')+'-'+$file.Name+'.gz')
            Save-Gzip $file.FullName (Join-Path $evidence $name)
        }
    }
    if(Test-Path "$temp/artifact.jsonl"){Save-Gzip "$temp/artifact.jsonl" "$evidence/temporary-artifact.jsonl.gz"}
    if(Test-Path "$temp/download.log"){Save-Gzip "$temp/download.log" "$evidence/temporary-download.log.gz"}
    if(Test-Path "$temp/range-download.log"){Save-Gzip "$temp/range-download.log" "$evidence/range-download.log.gz"}
    if(Test-Path "$temp/download-source.json"){Save-Gzip "$temp/download-source.json" "$evidence/download-source.json.gz"}
    }catch{
        $ok=$false
        $failure="Evidence export failed: $($_.Exception.Message)"
        throw
    }finally{
    $fp16Bytes=if(Test-Path "$models/models--Xenova--bge-m3"){(Get-ChildItem "$models/models--Xenova--bge-m3" -Recurse -File | Measure-Object Length -Sum).Sum}else{0}
    foreach($entry in $links){
        $item=Get-Item -LiteralPath $entry.path -Force -ErrorAction SilentlyContinue
        if(!$item){continue}
        if($item.LinkType -ne 'Junction' -or [IO.Path]::GetFullPath([string]@($item.Target)[0]) -ne $entry.target){throw 'Unexpected model link; refusing recursive cleanup'}
        [IO.Directory]::Delete($entry.path)
    }
    # The root is an owned UUID child of TEMP; shared directory links were removed non-recursively first.
    Remove-Item -LiteralPath $temp -Recurse -Force
    @{ok=$ok;phase=$phase;error=$failure;fp16_download_bytes=$fp16Bytes;shared_model_copies=0;temporary_root_removed=!(Test-Path -LiteralPath $temp);at=[DateTime]::UtcNow.ToString('o')} | ConvertTo-Json | Set-Content "$evidence/temporary-pipeline.json"
    }
}
if(!$ok){exit 1}
