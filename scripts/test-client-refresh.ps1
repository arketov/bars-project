param(
    [string]$BinDir = $(if ($env:CARGO_TARGET_DIR) { "$env:CARGO_TARGET_DIR/debug" } else { 'C:/bt/bars/debug' }),
    [string]$EvidenceDir = (Join-Path $PSScriptRoot ("../target/client-refresh-" + [guid]::NewGuid().ToString('N')))
)
$ErrorActionPreference = 'Stop'
$BinDir = [IO.Path]::GetFullPath($BinDir)
$EvidenceDir = [IO.Path]::GetFullPath($EvidenceDir)
$fixtureHome = Join-Path $EvidenceDir 'home'
$workerExe = Join-Path $BinDir 'examples/client_refresh_session.exe'
$cliExe = Join-Path $BinDir 'bars-cli.exe'
$mcpExe = Join-Path $BinDir 'bars-mcp.exe'
$repo = Split-Path $PSScriptRoot
New-Item -ItemType Directory -Path $fixtureHome -Force | Out-Null

function Assert-That($condition, [string]$message) {
    if (-not $condition) { throw $message }
}
function Write-Fixture([string]$path, [string]$text) {
    [IO.Directory]::CreateDirectory((Split-Path $path)) | Out-Null
    [IO.File]::WriteAllText($path, $text, [Text.UTF8Encoding]::new($false))
}
function New-Child([string]$exe, [string[]]$arguments, [string]$sessionHome) {
    $info = [Diagnostics.ProcessStartInfo]::new($exe)
    $info.UseShellExecute = $false
    $info.CreateNoWindow = $true
    $info.RedirectStandardInput = $true
    $info.RedirectStandardOutput = $true
    $info.RedirectStandardError = $true
    $info.WorkingDirectory = $sessionHome
    $info.Environment['BARS_USER_HOME'] = $sessionHome
    $info.Environment['CODEX_HOME'] = Join-Path $sessionHome '.codex'
    foreach ($argument in $arguments) { $info.ArgumentList.Add($argument) }
    $process = [Diagnostics.Process]::Start($info)
    return $process
}
function Invoke-Child([string]$exe, [string[]]$arguments, [string]$sessionHome = $fixtureHome) {
    $process = New-Child $exe $arguments $sessionHome
    $output = $process.StandardOutput.ReadToEndAsync()
    $errors = $process.StandardError.ReadToEndAsync()
    try {
        if (-not $process.WaitForExit(15000)) { throw "Child timeout: $exe" }
        Assert-That ($process.ExitCode -eq 0) ("Child failed: " + $errors.Result)
        return $output.Result
    } finally {
        if (-not $process.HasExited) { $process.Kill($true); $process.WaitForExit() }
        $process.Dispose()
    }
}
function Start-Session([string]$name, [string]$sessionHome = $fixtureHome) {
    $process = New-Child $workerExe @() $sessionHome
    $errors = $process.StandardError.ReadToEndAsync()
    $output = $process.StandardOutput.ReadToEndAsync()
    try {
        # CLI может успеть раньше bind и попытаться запустить обычный bars.exe.
        $deadline = [DateTime]::UtcNow.AddSeconds(15)
        do {
            try {
                $ready = Invoke-Child $cliExe @('projects', '--json') $sessionHome
                $null = $ready | ConvertFrom-Json
                break
            } catch {
                if ($process.HasExited -or [DateTime]::UtcNow -ge $deadline) { throw }
                Start-Sleep -Milliseconds 50
            }
        } while ($true)
        return @{ Process = $process; Errors = $errors; Output = $output; Name = $name; Home = $sessionHome }
    } catch {
        if (-not $process.HasExited) { $process.Kill($true); $process.WaitForExit() }
        Write-Fixture (Join-Path $EvidenceDir ($name + '.start-error.txt')) $errors.Result
        throw
    }
}
function Stop-Session($session) {
    Write-Fixture (Join-Path $session.Home 'stop') ''
    try {
        Assert-That ($session.Process.WaitForExit(15000)) 'Worker shutdown timeout'
        Assert-That ($session.Process.ExitCode -eq 0) $session.Errors.Result
        Write-Fixture (Join-Path $EvidenceDir ($session.Name + '.stderr.txt')) $session.Errors.Result
        # Продукт пишет интеграционные события через log в файл, не в stderr.
        $journalPath = Join-Path $session.Home '.bars/logs/worker.jsonl'
        $entries = @(if (Test-Path -LiteralPath $journalPath) {
            Get-Content -LiteralPath $journalPath | ForEach-Object { $_ | ConvertFrom-Json } |
                Where-Object { $_.pid -eq $session.Process.Id }
        })
        Write-Fixture (Join-Path $EvidenceDir ($session.Name + '.events.json')) (ConvertTo-Json -InputObject $entries -Depth 6)
        return ($entries.message -join "`n")
    } finally {
        if (-not $session.Process.HasExited) { $session.Process.Kill($true); $session.Process.WaitForExit() }
        $session.Process.Dispose()
        # Только созданный этой сессией сигнальный файл.
        [IO.File]::Delete((Join-Path $session.Home 'stop'))
    }
}
function Wait-Until([scriptblock]$predicate) {
    $deadline = [DateTime]::UtcNow.AddSeconds(15)
    do { if (& $predicate) { return }; Start-Sleep -Milliseconds 50 } while ([DateTime]::UtcNow -lt $deadline)
    throw 'Fixture update timeout'
}
function Get-Snapshot {
    $files = @('.claude.json','.claude/settings.json','.codex/config.toml','.codex/hooks.json',
        '.claude/skills/bars-docs/SKILL.md','.claude/skills/bars-session-log/SKILL.md','.claude/skills/bars-end/SKILL.md',
        '.agents/skills/bars-docs/SKILL.md','.agents/skills/bars-session-log/SKILL.md','.agents/skills/bars-end/SKILL.md')
    $snapshot = [ordered]@{}
    foreach ($file in $files) {
        foreach ($suffix in @('', '.bars-backup')) {
            $path = Join-Path $fixtureHome ($file + $suffix)
            if (Test-Path -LiteralPath $path) {
                $snapshot[$file + $suffix] = @((Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash, (Get-Item -LiteralPath $path).LastWriteTimeUtc.Ticks)
            }
        }
    }
    return ($snapshot | ConvertTo-Json -Depth 5 -Compress)
}
function Read-Mcp($process) {
    $read = $process.StandardOutput.ReadLineAsync()
    Assert-That ($read.Wait(10000)) 'MCP response timeout'
    Assert-That ($null -ne $read.Result) 'MCP closed stdout'
    return ($read.Result | ConvertFrom-Json -AsHashtable)
}

$active = $null
try {
    # Общая папка skills через ссылку (OQ-54), без доступа к настоящему профилю.
    New-Item -ItemType Directory -Path (Join-Path $fixtureHome 'shared-skills') -Force | Out-Null
    New-Item -ItemType Directory -Path (Join-Path $fixtureHome '.agents') -Force | Out-Null
    New-Item -ItemType Junction -Path (Join-Path $fixtureHome '.agents/skills') -Target (Join-Path $fixtureHome 'shared-skills') | Out-Null
    Write-Fixture (Join-Path $fixtureHome '.claude.json') '{"mcpServers":{"other":{"command":"other"},"bars":{"command":"old/bars-mcp"}},"foreign":42}'
    $oldHook = '{"hooks":{"SessionStart":[{"matcher":"clear","hooks":[{"type":"command","command":"foreign"},{"type":"prompt","command":"old/bars-cli hook session-start"}]},{"hooks":[{"command":"bars-cli hook session-start"}]}]}}'
    Write-Fixture (Join-Path $fixtureHome '.claude/settings.json') $oldHook
    Write-Fixture (Join-Path $fixtureHome '.codex/config.toml') "# foreign comment`n[mcp_servers.other]`ncommand = 'other'`n[mcp_servers.bars]`ncommand = 'old/bars-mcp'`n"
    Write-Fixture (Join-Path $fixtureHome '.codex/hooks.json') $oldHook
    foreach ($base in @('.agents/skills','.claude/skills')) {
        foreach ($skill in @('bars-docs','bars-session-log','bars-end')) {
            Write-Fixture (Join-Path $fixtureHome "$base/$skill/SKILL.md") 'old skill text'
        }
    }
    Write-Fixture (Join-Path $fixtureHome '.agents/skills/foreign/SKILL.md') 'foreign'
    $active = Start-Session '01-update'
    Wait-Until {
        foreach ($skill in @('bars-docs','bars-session-log','bars-end')) {
            $expected = [IO.File]::ReadAllText((Join-Path $repo "crates/bars-worker/assets/skills/$skill/SKILL.md"))
            foreach ($base in @('.agents/skills','.claude/skills')) {
                $path = Join-Path $fixtureHome "$base/$skill/SKILL.md"
                if (-not (Test-Path -LiteralPath $path) -or [IO.File]::ReadAllText($path) -cne $expected) { return $false }
            }
        }
        return $true
    }
    foreach ($skill in @('bars-docs','bars-session-log','bars-end')) {
        $expected = [IO.File]::ReadAllText((Join-Path $repo "crates/bars-worker/assets/skills/$skill/SKILL.md"))
        foreach ($base in @('.agents/skills','.claude/skills')) {
            Assert-That ([IO.File]::ReadAllText((Join-Path $fixtureHome "$base/$skill/SKILL.md")) -ceq $expected) "Skill mismatch: $base/$skill"
        }
    }
    $claude = Get-Content -Raw (Join-Path $fixtureHome '.claude.json') | ConvertFrom-Json -AsHashtable
    Assert-That ($claude.foreign -eq 42 -and $claude.mcpServers.other.command -eq 'other') 'Foreign MCP lost'
    $hooks = Get-Content -Raw (Join-Path $fixtureHome '.codex/hooks.json') | ConvertFrom-Json -AsHashtable
    Assert-That ($hooks.hooks.SessionStart.Count -eq 2) 'Duplicate hooks remain'
    Assert-That ($hooks.hooks.SessionStart[0].matcher -eq 'clear' -and $hooks.hooks.SessionStart[0].hooks[0].command -eq 'foreign') 'Foreign hook changed'
    $codex = Get-Command codex -ErrorAction SilentlyContinue
    if ($codex) {
        $codexRead = Invoke-Child $codex.Source @('mcp','get','bars','--json')
        Write-Fixture (Join-Path $EvidenceDir 'codex-mcp-get.json') $codexRead
        $codexConfig = $codexRead | ConvertFrom-Json -AsHashtable
        Assert-That ($codexConfig.transport.command -eq $mcpExe) 'Codex reads wrong command'
    }
    $mcp = New-Child $mcpExe @() $fixtureHome
    $mcpErrors = $mcp.StandardError.ReadToEndAsync()
    try {
        $mcp.StandardInput.WriteLine('{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"stage14-fixture","version":"1"}}}')
        $initialized = Read-Mcp $mcp
        Assert-That ($initialized.id -eq 1 -and $null -ne $initialized.result) 'MCP initialize failed'
        $mcp.StandardInput.WriteLine('{"jsonrpc":"2.0","method":"notifications/initialized"}')
        $mcp.StandardInput.WriteLine('{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"bars_projects","arguments":{}}}')
        $projects = Read-Mcp $mcp
        Assert-That ($projects.id -eq 2 -and $null -ne $projects.result -and -not $projects.result.isError) 'MCP bars_projects failed'
        Write-Fixture (Join-Path $EvidenceDir 'mcp-projects.json') ($projects | ConvertTo-Json -Depth 15)
        $mcp.StandardInput.Close()
        Assert-That ($mcp.WaitForExit(10000)) 'MCP did not close'
    } finally {
        if (-not $mcp.HasExited) { $mcp.Kill($true); $mcp.WaitForExit() }
        $mcp.Dispose()
    }
    $log = Stop-Session $active; $active = $null
    Assert-That (([regex]::Matches($log, '/hooks')).Count -eq 1) 'Approval reminder count'
    $before = Get-Snapshot
    $active = Start-Session '02-noop'
    $log = Stop-Session $active; $active = $null
    Assert-That ((Get-Snapshot) -ceq $before) 'No-op rewrote bytes, mtime or backup'
    Assert-That ($log -notmatch 'обновлено|/hooks') 'No-op update reported changes'
    $active = Start-Session '03-uninstall'
    $null = Invoke-Child $cliExe @('uninstall','--client','claude-code','--json')
    $null = Invoke-Child $cliExe @('uninstall','--client','codex','--json')
    $null = Stop-Session $active; $active = $null
    $before = Get-Snapshot
    $active = Start-Session '04-after-uninstall'
    $log = Stop-Session $active; $active = $null
    Assert-That ((Get-Snapshot) -ceq $before) 'Uninstalled client resurrected'
    Assert-That ($log -notmatch 'обновлено|/hooks') 'Uninstalled client updated'
    Assert-That (Test-Path (Join-Path $fixtureHome '.agents/skills/foreign/SKILL.md')) 'Foreign skill removed'
    Write-Fixture (Join-Path $fixtureHome '.claude/settings.json') '{broken'
    Write-Fixture (Join-Path $fixtureHome '.claude.json') '{"mcpServers":{"bars":{"command":"old"}}}'
    Write-Fixture (Join-Path $fixtureHome '.codex/config.toml') "[mcp_servers.bars]`ncommand = 'old'`n"
    $beforeClaude = [IO.File]::ReadAllText((Join-Path $fixtureHome '.claude.json'))
    $active = Start-Session '05-error-isolation'
    $log = Stop-Session $active; $active = $null
    Assert-That ($log -match 'Claude Code: ошибка' -and $log -match 'Codex: обновлено') 'Error isolation failed'
    Assert-That ([IO.File]::ReadAllText((Join-Path $fixtureHome '.claude.json')) -ceq $beforeClaude) 'Preflight modified MCP'
    $emptyHome = Join-Path $EvidenceDir 'empty-home'
    New-Item -ItemType Directory -Path $emptyHome | Out-Null
    $active = Start-Session '06-empty' $emptyHome
    $log = Stop-Session $active; $active = $null
    Assert-That (-not (Test-Path (Join-Path $emptyHome '.codex')) -and -not (Test-Path (Join-Path $emptyHome '.claude'))) 'Empty client installed'
    $summary = @{ sessions = 6; passed = $true; codex_config_read = [bool]$codex; mcp_stdio = $true; skills_junction = $true; evidence = $EvidenceDir }
    $summary | ConvertTo-Json | Tee-Object -FilePath (Join-Path $EvidenceDir 'summary.json')
} finally {
    if ($null -ne $active) { $null = Stop-Session $active }
}
