# Проверки перед коммитом: форматирование, clippy, сборка и тесты workspace, затем проверки окна.
# Описание — docs/knowledge/build.md. Код выхода 0 только если зелёные все шаги.
param(
    [int]$Jobs = 3,
    [switch]$SkipUi
)
$ErrorActionPreference = 'Stop'
$repo = Split-Path $PSScriptRoot
Set-Location $repo

# Кэш сборки один — C:/bt/bars (docs/knowledge/build.md): в target/ репозитория длинный путь ломает llama.cpp.
if (-not $env:CARGO_TARGET_DIR) { $env:CARGO_TARGET_DIR = 'C:/bt/bars' }
# Без LIBCLANG_PATH bindgen в llama-cpp-sys-2 не находит libclang.
if (-not $env:LIBCLANG_PATH -and (Test-Path 'C:\Program Files\LLVM\bin')) { $env:LIBCLANG_PATH = 'C:\Program Files\LLVM\bin' }
$env:CMAKE_GENERATOR = 'Ninja'
$env:CMAKE_BUILD_PARALLEL_LEVEL = '1'

$results = [System.Collections.Generic.List[object]]::new()
function Invoke-Step([string]$Name, [scriptblock]$Command, [string]$Dir = $repo) {
    Write-Host "`n=== $Name ===" -ForegroundColor Cyan
    $started = Get-Date
    Push-Location $Dir
    try { & $Command; $code = $LASTEXITCODE } finally { Pop-Location }
    $seconds = [int]((Get-Date) - $started).TotalSeconds
    $results.Add([pscustomobject]@{ Step = $Name; Code = $code; Seconds = $seconds })
    if ($code -ne 0) {
        Write-Host "`nПровалено: $Name (код $code). Коммитить нельзя." -ForegroundColor Red
        $results | Format-Table -AutoSize
        exit $code
    }
}

# -j ограничивает одновременную линковку: при числе заданий по умолчанию link.exe на Windows падает без текста ошибки (ISS-65).
# --lib --tests --bins не собирает примеры, которые никто не запускает.
Invoke-Step 'cargo fmt --check' { cargo fmt --all --check }
Invoke-Step 'cargo clippy' { cargo clippy -j $Jobs --workspace --all-targets -- -D warnings }
Invoke-Step 'cargo build' { cargo build -j $Jobs --workspace }
Invoke-Step 'cargo test' { cargo test -j $Jobs --workspace --lib --tests --bins --no-fail-fast }

if (-not $SkipUi) {
    $ui = Join-Path $repo 'ui'
    Invoke-Step 'ui: npm run check' { npm run check } $ui
    Invoke-Step 'ui: npm test' { npm test } $ui
}

Write-Host "`nВсе проверки зелёные." -ForegroundColor Green
$results | Format-Table -AutoSize
exit 0
