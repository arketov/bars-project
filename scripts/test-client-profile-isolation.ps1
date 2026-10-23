# ISS-12: тестовые воркеры не должны менять настоящий профиль клиента.
$ErrorActionPreference = 'Stop'
if ($env:BARS_USER_HOME) { throw 'Run without BARS_USER_HOME to verify the real profile stays unchanged' }
$cargo = (Get-Command cargo -ErrorAction SilentlyContinue).Source
if (-not $cargo) { $cargo = Join-Path $env:USERPROFILE '.cargo/bin/cargo.exe' }
if (-not (Test-Path -LiteralPath $cargo)) { throw 'cargo not found' }
# Кэш сборки один — C:/bt/bars (docs/knowledge/build.md).
if (-not $env:CARGO_TARGET_DIR) { $env:CARGO_TARGET_DIR = 'C:/bt/bars' }

function Get-ProfileSnapshot {
    $files = @(
        '.codex/config.toml', '.codex/hooks.json',
        '.codex/config.toml.bars-backup', '.codex/hooks.json.bars-backup',
        '.claude.json', '.claude/settings.json',
        '.claude.json.bars-backup', '.claude/settings.json.bars-backup'
    ) | ForEach-Object { Join-Path $env:USERPROFILE $_ }
    foreach ($base in @('.agents/skills', '.claude/skills')) {
        foreach ($skill in @('bars-docs', 'bars-session-log')) {
            $path = Join-Path $env:USERPROFILE "$base/$skill"
            if (Test-Path -LiteralPath $path) {
                $files += Get-ChildItem -LiteralPath $path -File -Recurse | ForEach-Object FullName
            }
        }
    }
    foreach ($path in ($files | Sort-Object -Unique)) {
        if (Test-Path -LiteralPath $path) {
            $item = Get-Item -LiteralPath $path
            $hash = (Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash
            "$path|$hash|$($item.LastWriteTimeUtc.Ticks)"
        } else {
            "$path|absent"
        }
    }
}

Push-Location (Split-Path $PSScriptRoot)
try {
    $before = @(Get-ProfileSnapshot)
    try {
        # Старая реализация падает до serve, без записи в пользовательский профиль.
        & $cargo test -p bars-worker --lib default_worker_does_not_enable_client_profile_writes
        if ($LASTEXITCODE -ne 0) { throw 'Profile isolation preflight failed' }
        & $cargo test -p bars-worker -p bars-cli -p bars-mcp --lib --tests
        if ($LASTEXITCODE -ne 0) { throw 'Client/worker regression tests failed' }
    } finally {
        $changes = @(Compare-Object $before @(Get-ProfileSnapshot))
        if ($changes.Count) {
            $changes | ForEach-Object { Write-Warning $_.InputObject }
            throw 'Client profile changed during regression tests'
        }
        Write-Host 'Client profile unchanged: SHA256, timestamps and file presence verified.'
    }
} finally {
    Pop-Location
}
