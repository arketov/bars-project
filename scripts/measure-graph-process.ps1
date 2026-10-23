param([string]$Executable='C:/bt/bars/release/examples/graph_acceptance.exe',[string]$Project='C:/Users/arketov/Desktop/bars-project',[string]$Output='target/stage33-probe/real-corpus-v2')
$probeRoot=(Get-Location).Path
$probeOutput=[IO.Path]::GetFullPath((Join-Path $probeRoot $Output))
if(-not $probeOutput.StartsWith($probeRoot+[IO.Path]::DirectorySeparatorChar)){throw 'Результат должен быть внутри checkout'}
if(Test-Path -LiteralPath $probeOutput){throw 'Нужна новая папка результата'}
$probeLog=Join-Path $probeRoot 'target/stage33-probe/process-measure.log'
$probeError=Join-Path $probeRoot 'target/stage33-probe/process-measure-error.log'
$probeBefore=Get-CimInstance Win32_OperatingSystem | Select-Object TotalVisibleMemorySize,FreePhysicalMemory
$probeProcess=Start-Process -FilePath $Executable -ArgumentList @($Project,$probeOutput) -WindowStyle Hidden -PassThru -RedirectStandardOutput $probeLog -RedirectStandardError $probeError
$probeSamples=[Collections.Generic.List[object]]::new()
$probeWatch=[Diagnostics.Stopwatch]::StartNew()
do {try{$probeProcess.Refresh();$probeSamples.Add([pscustomobject]@{elapsed_ms=$probeWatch.Elapsed.TotalMilliseconds;private_bytes=$probeProcess.PrivateMemorySize64;working_set=$probeProcess.WorkingSet64})}catch{};Start-Sleep -Milliseconds 20}while(-not $probeProcess.HasExited)
$probeProcess.WaitForExit()
if($probeProcess.ExitCode -ne 0){Get-Content -LiteralPath $probeError;throw "Probe exit $($probeProcess.ExitCode)"}
@{pid=$probeProcess.Id;executable_sha256=(Get-FileHash -LiteralPath $Executable -Algorithm SHA256).Hash;processor=$env:PROCESSOR_IDENTIFIER;before=$probeBefore;sample_interval_ms=20;elapsed_ms=$probeWatch.Elapsed.TotalMilliseconds;samples=$probeSamples;installed_bars_stopped=$false;quiet_machine=$false}|ConvertTo-Json -Depth 5|Set-Content -LiteralPath (Join-Path $probeOutput 'process.json') -Encoding utf8
Get-Content -LiteralPath $probeLog
