$ErrorActionPreference = 'Stop'
$barsRoot = (Resolve-Path (Join-Path $PSScriptRoot '../../..')).Path
$overdriveRoot = 'C:/Users/arketov/Desktop/cyberProject/ai-mutiplayer-bots'
$cases = @(
    @('B1', 'bars', 'features/models/local-models.md', 'FP16 требует отдельного индекса'),
    @('B2', 'bars', 'features/models/local-models.md', 'Смена устройства не меняет отпечаток'),
    @('B3', 'bars', 'features/window/reindex-consent.md', 'агент через MCP согласие дать не может'),
    @('B4', 'bars', 'features/indexing/document-queue.md', 'DocQueue'),
    @('B5', 'bars', 'architecture/graph.md', 'Это не повод повторять'),
    @('B6', 'bars', 'knowledge/graph-render.md', 'не подтверждают физическую приёмку'),
    @('B7', 'bars', 'backlog/issues.md', 'возвращает пустой список'),
    @('O1', 'overdrive', 'features/bot-groups/requirements.md', 'Группы — временное состояние'),
    @('O2', 'overdrive', 'features/bot-groups/requirements.md', 'не назначает цель Follow/Police'),
    @('O3', 'overdrive', 'features/server-connection/multi-server.md', 'Переключение не запускает и не отключает'),
    @('O4', 'overdrive', 'features/runtime-capacity/limits-and-scaling.md', 'При одном разрешении'),
    @('O5', 'overdrive', 'adr/0036-multi-server-single-runtime.md', 'дословный ответ'),
    @('O6', 'overdrive', 'features/settings-and-game-setup/preparation-and-restore.md', 'текущий SHA-256'),
    @('O7', 'overdrive', 'features/server-connection/multi-server.md', 'Эти проверки не подтверждают')
)
$rows = foreach ($case in $cases) {
    $root = if ($case[1] -eq 'bars') { $barsRoot } else { $overdriveRoot }
    $source = Join-Path $root ('docs/' + $case[2])
    $lines = Get-Content -LiteralPath $source -Encoding utf8
    $matches = @(for ($i = 0; $i -lt $lines.Count; $i++) {
        if ($lines[$i].Contains($case[3])) {
            [ordered]@{ line = $i + 1; text = $lines[$i] }
        }
    })
    if ($matches.Count -eq 0) { throw "Missing evidence: $($case[0])" }
    [ordered]@{
        id = $case[0]; project = $case[1]; path = 'docs/' + $case[2]
        sha256 = (Get-FileHash -LiteralPath $source -Algorithm SHA256).Hash
        evidence = $matches
    }
}
$routes = @(
    @('bars', 'features/window/reindex-consent.md', '../../architecture/protocol.md'),
    @('overdrive', 'features/runtime-capacity/limits-and-scaling.md', '../server-connection/multi-server.md'),
    @('overdrive', 'features/server-connection/multi-server.md', '../../adr/0036-multi-server-single-runtime.md')
)
$links = foreach ($route in $routes) {
    $root = if ($route[0] -eq 'bars') { $barsRoot } else { $overdriveRoot }
    $source = Join-Path $root ('docs/' + $route[1])
    $raw = Get-Content -LiteralPath $source -Raw -Encoding utf8
    if (-not $raw.Contains('](' + $route[2] + ')')) { throw "Missing link: $source" }
    $target = Join-Path (Split-Path $source) $route[2]
    if (-not (Test-Path -LiteralPath $target -PathType Leaf)) { throw "Missing target: $target" }
    [ordered]@{ project = $route[0]; source = $route[1]; target = $route[2]; verified = $true }
}
[ordered]@{
    date = '2026-10-26'
    method = 'Ручной сценарный разбор; скрипт проверяет буквальные основания и Markdown-переходы, не качество retrieval или UI.'
    cases = $rows
    links = $links
} | ConvertTo-Json -Depth 8 | Set-Content -LiteralPath (Join-Path $PSScriptRoot 'source-evidence.json') -Encoding utf8
Write-Output "Verified $($rows.Count) cases and $($links.Count) links."
