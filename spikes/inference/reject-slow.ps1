$ErrorActionPreference='Stop'
$expectedExe=(Join-Path $env:LOCALAPPDATA 'bars/target/release/examples/bench_inference.exe') -replace '/','\'
foreach($name in @('cpu-t1-b4','cpu-t2-b4')){
    $dir="spikes/inference/runs/screen/$name"
    while($true){
        if(Test-Path "$dir/complete.json"){break}
        if(Test-Path "$dir/rows.jsonl"){
            $rows=@(Get-Content "$dir/rows.jsonl" | ForEach-Object {$_ | ConvertFrom-Json})
            if($rows.Count -ge 3){
                $first=@($rows | Select-Object -First 3);$times=@($first.seconds | Sort-Object);$median=$times[1]
                if($median -gt 2*8.285){
                    $owned=@(Get-CimInstance Win32_Process -Filter "Name='bench_inference.exe'" | Where-Object {$_.CommandLine.Contains("$name.json")})
                    if($owned.Count -ne 1){throw 'Owned process is ambiguous'}
                    $process=Get-Process -Id $owned[0].ProcessId
                    if($process.Path -ne $expectedExe){throw 'Unexpected process path'}
                    @{status='rejected-slow-pilot';baseline_median=8.285;cutoff_multiplier=2;observations=$first.Count;median=$median;policy='After three warm observations, reject configuration over twice CPU16 baseline; not a complete quality run.';at=[DateTime]::UtcNow.ToString('o')} | ConvertTo-Json | Set-Content "$dir/rejected.json"
                    Stop-Process -Id $process.Id
                    Write-Output "Rejected $name median=$median after $($first.Count) warm observations"
                }
                break
            }
        }
        Start-Sleep -Seconds 2
    }
}
