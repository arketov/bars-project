param(
    [Parameter(Mandatory)][string]$Url,
    [Parameter(Mandatory)][long]$Bytes,
    [Parameter(Mandatory)][ValidatePattern('^[a-f0-9]{64}$')][string]$Sha256,
    [Parameter(Mandatory)][string]$Destination
)
$ErrorActionPreference = 'Stop'
$dest = [IO.Path]::GetFullPath($Destination)
if (Test-Path -LiteralPath $dest) { throw 'Назначение уже существует' }
$parts = "$dest.parts"
New-Item -ItemType Directory -Force $parts | Out-Null
$count = [int][Math]::Ceiling($Bytes / 16777216.0)
$results = 0..($count-1) | ForEach-Object -Parallel {
    $ErrorActionPreference = 'Stop'
    $part = $_; $start = [long]$part * 16777216
    $end = [Math]::Min($start + 16777215, $using:Bytes - 1)
    $path = Join-Path $using:parts "$part"
    if (-not (Test-Path -LiteralPath $path) -or (Get-Item -LiteralPath $path).Length -ne $end-$start+1) {
        & curl.exe -L --fail --max-time 180 --retry 2 --silent --show-error --range "$start-$end" "$using:Url`?download=true&part=$part" -o $path
        if ($LASTEXITCODE) { throw "Ошибка скачивания части $part" }
    }
    if ((Get-Item -LiteralPath $path).Length -ne $end-$start+1) { throw "Неверная длина части $part" }
    $part
} -ThrottleLimit 4
if (@($results).Count -ne $count) { throw 'Не все части готовы' }
$stream = [IO.File]::CreateNew($dest)
try {
    foreach ($i in 0..($count-1)) {
        $inputPart = [IO.File]::OpenRead((Join-Path $parts "$i"))
        try { $inputPart.CopyTo($stream) } finally { $inputPart.Dispose() }
    }
} finally { $stream.Dispose() }
if ((Get-FileHash -LiteralPath $dest -Algorithm SHA256).Hash.ToLowerInvariant() -ne $Sha256) { throw 'SHA-256 не совпал; файл непригоден' }
Write-Output "Проверен $dest ($Bytes байт)"
