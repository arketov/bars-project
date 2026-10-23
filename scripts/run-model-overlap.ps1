param(
    [Parameter(Mandatory)][string]$Executable,
    [string]$Models = (Join-Path $env:USERPROFILE '.bars/models'),
    [string]$CorpusBundle = 'docs/knowledge/memory-2026-10-23/recovery-fp16/corpus-focused.json.gz',
    [string]$Evidence = 'docs/chats/2026-10-23-index-memory-policy',
    [ValidateSet('fp32','fp16')][string]$Precision = 'fp32',
    [ValidateRange(1,600)][int]$WaitSeconds = 90
)
$ErrorActionPreference = 'Stop'
$exe = (Resolve-Path -LiteralPath $Executable).Path
$cache = (Resolve-Path -LiteralPath $Models).Path
$bundle = (Resolve-Path -LiteralPath $CorpusBundle).Path
$output = (Resolve-Path -LiteralPath $Evidence).Path
$id = [Guid]::NewGuid().ToString('N')
$tempParent = [IO.Path]::GetFullPath([IO.Path]::GetTempPath()).TrimEnd('\','/')
$owned = Join-Path $tempParent "bars-memory-policy-$id"
$report = Join-Path $output "$Precision-$id"
New-Item -ItemType Directory -Path $owned,$report | Out-Null
$phase = 'preflight'
$child = $null
$ok = $false
try {
    @{binary_sha256=(Get-FileHash -LiteralPath $exe).Hash;corpus_sha256=(Get-FileHash -LiteralPath $bundle).Hash;precision=$Precision;started_utc=[DateTime]::UtcNow.ToString('o')} | ConvertTo-Json | Set-Content -LiteralPath "$report/provenance.json"
    foreach($mode in 'serial','overlap') {
        $phase = "$mode-preflight"
        $ready = 0
        $deadline = [DateTime]::UtcNow.AddSeconds($WaitSeconds)
        do {
            $memory = Get-CimInstance Win32_PerfFormattedData_PerfOS_Memory
            $freeCommit = [long]$memory.CommitLimit - [long]$memory.CommittedBytes
            $enough = $memory.AvailableBytes -ge 4GB -and $freeCommit -ge 8GB
            @{utc=[DateTime]::UtcNow.ToString('o');phase=$phase;available_bytes=$memory.AvailableBytes;free_commit_bytes=$freeCommit;required_commit_bytes=8GB;ready=$enough} | ConvertTo-Json -Compress | Add-Content -LiteralPath "$report/preflight.jsonl"
            if($enough){$ready++}else{$ready=0}
            if($ready -ge 3){break}
            if([DateTime]::UtcNow -ge $deadline){throw 'Недостаточный естественный запас commit; модель не запущена'}
            Start-Sleep -Seconds 2
        } while($true)
        # Independent fresh copies: no persisted index leaks between modes.
        $project = Join-Path $owned "$mode-project"
        New-Item -ItemType Directory -Path $project | Out-Null
        & node scripts/prepare-index-memory-corpus.mjs unpack $bundle "$project/docs" "$report/$mode-corpus.json"
        if($LASTEXITCODE -ne 0){throw 'Проверка замороженного корпуса не прошла'}
        $phase = $mode
        $run = Join-Path $owned "$mode-run"
        $arguments = @($Precision,'on','1',$cache,$project,$run,$mode) | ForEach-Object {'"'+$_+'"'}
        $child = Start-Process -FilePath $exe -ArgumentList $arguments -WindowStyle Hidden -PassThru -RedirectStandardOutput "$report/$mode.stdout.txt" -RedirectStandardError "$report/$mode.stderr.txt"
        $runDeadline = [DateTime]::UtcNow.AddMinutes(25)
        while(-not $child.WaitForExit(1000)) {
            if([DateTime]::UtcNow -ge $runDeadline){throw 'Собственный стенд превысил 25 минут'}
        }
        $child.WaitForExit()
        $exitCode = $child.ExitCode
        @{pid=$child.Id;exit_code=$exitCode} | ConvertTo-Json | Set-Content -LiteralPath "$report/$mode-process.json"
        $child = $null
        if($exitCode -ne 0){throw "Стенд $mode завершился с кодом $exitCode; не принят"}
    }
    $ok = $true
} catch {
    @{phase=$phase;error=$_.Exception.Message} | ConvertTo-Json | Set-Content -LiteralPath "$report/failure.json"
    throw
} finally {
    if($child -and -not $child.HasExited){Stop-Process -Id $child.Id; $child.WaitForExit()}
    # Export partial runs on timeout/guard failure too.
    foreach($savedMode in 'serial','overlap') {
        foreach($name in 'memory.jsonl','ort.jsonl') {
            $source = Join-Path $owned "$savedMode-run/$name"
            if(Test-Path -LiteralPath $source) {
                $inputStream = [IO.File]::OpenRead($source)
                $outputStream = [IO.File]::Create("$report/$savedMode-$name.gz")
                $gzip = [IO.Compression.GZipStream]::new($outputStream,[IO.Compression.CompressionMode]::Compress)
                try {$inputStream.CopyTo($gzip)} finally {$gzip.Dispose();$outputStream.Dispose();$inputStream.Dispose()}
            }
        }
    }
    # Only the UUID directory this invocation created; shared cache is never removed.
    $resolved = [IO.Path]::GetFullPath($owned)
    if([IO.Path]::GetDirectoryName($resolved) -ne $tempParent -or [IO.Path]::GetFileName($resolved) -ne "bars-memory-policy-$id"){throw 'Unsafe cleanup target'}
    Remove-Item -LiteralPath $resolved -Recurse -Force
    @{ok=$ok;last_phase=$phase;temporary_removed=(-not(Test-Path -LiteralPath $resolved));finished_utc=[DateTime]::UtcNow.ToString('o')} | ConvertTo-Json | Set-Content -LiteralPath "$report/result.json"
}
