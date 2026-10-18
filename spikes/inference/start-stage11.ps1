$ErrorActionPreference='Stop'
$exe=Join-Path $env:LOCALAPPDATA 'bars/target/release/examples/bench_inference.exe'
$models=Join-Path $env:USERPROFILE '.bars/models'
$quiet=0
while($quiet -lt 4){
    if(@(Get-Process bench_memory,bench_quality,bench_inference,rustc -ErrorAction SilentlyContinue).Count){$quiet=0}else{$quiet++}
    Start-Sleep -Seconds 5
}
$onnx=(Get-ChildItem "$models/models--rozgo--bge-reranker-v2-m3/snapshots/*/model.onnx" | Select-Object -First 1).FullName
foreach($name in @('dml-b4','cpu-t16-b4')){
    $out="spikes/inference/runs/profile-$name"
    if(!(Test-Path $out)){
        & $exe profile spikes/inference/data/overdrive $models "spikes/inference/configs/$name.json" $onnx $out
        if($LASTEXITCODE){throw "Profile failed: $name"}
        Write-Output "Profile complete: $name"
    }
}
$configs=@('dml-b4','cpu-t16-b4','cpu-t1-b4','cpu-t2-b4','cpu-t4-b4','cpu-t8-b4','dml-b4-sorted','dml-b1','dml-b2','dml-b8','dml-b16') | ForEach-Object {"spikes/inference/configs/$_.json"}
& "$PSScriptRoot/run-series.ps1" -Executable $exe -Data spikes/inference/data/overdrive -Models $models -Configs $configs -OutputRoot spikes/inference/runs/screen
