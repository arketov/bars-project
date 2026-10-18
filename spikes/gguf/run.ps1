param(
    [Parameter(Mandatory)][string]$Binary,
    [Parameter(Mandatory)][string]$Model,
    [Parameter(Mandatory)][ValidateSet('cpu','gpu','bge','prod-cpu','prod-gpu')][string]$Device,
    [Parameter(Mandatory)][string]$Project,
    [Parameter(Mandatory)][string]$Cases,
    [Parameter(Mandatory)][string]$Output,
    [string]$ModelsCache,
    [switch]$EnforceReserve,
    [int]$MinCommitMiB = 4096
)
$ErrorActionPreference = 'Stop'
$exe = (Resolve-Path -LiteralPath $Binary).Path
$out = [IO.Path]::GetFullPath($Output)
$memory = Get-CimInstance Win32_OperatingSystem
if ($EnforceReserve -and ($memory.FreeVirtualMemory / 1024 -lt $MinCommitMiB -or $memory.FreePhysicalMemory / 1024 -lt 4096)) { throw 'Недостаточный запас RAM/commit для прогона' }
$benchArgs = @('bench', $Model, $Device, $Project, $Cases, $out)
if ($ModelsCache) { $benchArgs += $ModelsCache }
$argsList = $benchArgs | ForEach-Object {
    if ($_ -match '"') { throw 'Кавычка в аргументе не поддерживается' }
    '"' + $_ + '"'
}
$p = Start-Process -FilePath $exe -ArgumentList $argsList -WindowStyle Hidden -PassThru -RedirectStandardOutput "$out.stdout" -RedirectStandardError "$out.stderr"
$started = Get-Date
try {
    while (-not $p.WaitForExit(500)) {
        $p.Refresh()
        [ordered]@{utc=[DateTime]::UtcNow.ToString('o');pid=$p.Id;private=$p.PrivateMemorySize64;working=$p.WorkingSet64;peak_working=$p.PeakWorkingSet64} | ConvertTo-Json -Compress | Add-Content "$out.memory.jsonl"
        if ((Get-Date) - $started -gt [TimeSpan]::FromMinutes(30)) { throw 'Превышено 30 минут' }
        if ($EnforceReserve) {
            $memory = Get-CimInstance Win32_OperatingSystem
            if ($memory.FreeVirtualMemory / 1024 -lt 1024 -or $memory.FreePhysicalMemory / 1024 -lt 2048) { throw 'Прогон прерван: недостаточный запас RAM/commit' }
        }
    }
    if ($p.ExitCode -ne 0) { throw "Спайк завершился с кодом $($p.ExitCode); см. $out.stderr" }
} finally {
    if (-not $p.HasExited) { Stop-Process -Id $p.Id }
}
Write-Output "Готово: $out"
