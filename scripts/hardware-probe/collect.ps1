param([ValidateRange(1,100)][int]$Runs = 20)
$ErrorActionPreference = 'Stop'
$repo = Split-Path (Split-Path $PSScriptRoot)
$destination = Join-Path $repo 'docs/knowledge/hardware-discovery-2026-11-02'
New-Item -ItemType Directory -Force -Path $destination | Out-Null
$probe = 'C:/bt/bars/debug/bars-hardware-probe.exe'
if (-not (Test-Path -LiteralPath $probe)) { throw 'Сначала соберите hardware-probe с CARGO_TARGET_DIR=C:/bt/bars' }
$rows = [System.Collections.Generic.List[string]]::new()
for ($i = 0; $i -lt $Runs; $i++) {
    $info = [Diagnostics.ProcessStartInfo]::new($probe)
    $info.UseShellExecute = $false
    $info.CreateNoWindow = $true
    $info.RedirectStandardOutput = $true
    $info.RedirectStandardError = $true
    $process = [Diagnostics.Process]::Start($info)
    try {
        # Читать оба pipe асинхронно: заполнение pipe не должно блокировать таймаут.
        $stdout = $process.StandardOutput.ReadToEndAsync()
        $stderr = $process.StandardError.ReadToEndAsync()
        if (-not $process.WaitForExit(10000)) {
            $process.Kill($true)
            $process.WaitForExit()
            throw "Истёк срок 10 секунд, остановлен только собственный probe, прогон $i"
        }
        $line = $stdout.GetAwaiter().GetResult().Trim()
        if ($process.ExitCode -ne 0) { throw $stderr.GetAwaiter().GetResult() }
        $null = $line | ConvertFrom-Json
        $rows.Add($line)
    } finally { $process.Dispose() }
}
[IO.File]::WriteAllLines((Join-Path $destination 'windows-runs.jsonl'), $rows)
$independent = [ordered]@{
    captured_at = [DateTimeOffset]::UtcNow.ToString('o')
    os = Get-CimInstance Win32_OperatingSystem | Select-Object Caption,Version,BuildNumber
    cpu = Get-CimInstance Win32_Processor | Select-Object Name
    installed_ram_bytes = (Get-CimInstance Win32_PhysicalMemory | Measure-Object Capacity -Sum).Sum
    wmi_gpu = @(Get-CimInstance Win32_VideoController | Select-Object Name,AdapterRAM,DriverVersion)
    nvidia_smi = if (Test-Path -LiteralPath "$env:SystemRoot/System32/nvidia-smi.exe") { & "$env:SystemRoot/System32/nvidia-smi.exe" --query-gpu=name,memory.total,driver_version --format=csv,noheader } else { 'not_available' }
    rustc = & "$env:USERPROFILE/.cargo/bin/rustc.exe" --version
    probe_sha256 = (Get-FileHash -LiteralPath $probe -Algorithm SHA256).Hash.ToLowerInvariant()
    source_sha256 = (Get-FileHash -LiteralPath (Join-Path $PSScriptRoot 'src/main.rs') -Algorithm SHA256).Hash.ToLowerInvariant()
    manifest_sha256 = (Get-FileHash -LiteralPath (Join-Path $PSScriptRoot 'Cargo.toml') -Algorithm SHA256).Hash.ToLowerInvariant()
    lock_sha256 = (Get-FileHash -LiteralPath (Join-Path $PSScriptRoot 'Cargo.lock') -Algorithm SHA256).Hash.ToLowerInvariant()
}
$independent | ConvertTo-Json -Depth 6 | Set-Content -LiteralPath (Join-Path $destination 'independent.json') -Encoding utf8
$data = @($rows | ForEach-Object { $_ | ConvertFrom-Json })
$summary = foreach ($stage in @('total','ram','dxgi','dxcore','cuda','toolkits')) {
    $values = @($data | ForEach-Object { if ($stage -eq 'total') { $_.elapsed_us / 1000 } else { $_.$stage.elapsed_us / 1000 } } | Sort-Object)
    [pscustomobject]@{stage=$stage; runs=$Runs; min_ms=$values[0]; median_ms=($values[[int][Math]::Floor(($Runs-1)/2)] + $values[[int][Math]::Floor($Runs/2)])/2; p95_ms=$values[[int][Math]::Ceiling($Runs*0.95)-1]; max_ms=$values[-1]}
}
$summary | ConvertTo-Json | Set-Content -LiteralPath (Join-Path $destination 'timing-summary.json') -Encoding utf8
$summary | Format-Table -AutoSize
