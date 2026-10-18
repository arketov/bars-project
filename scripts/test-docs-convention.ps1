# Изолирует также библиотечные Cargo-тесты, а не только процессные стенды установки.
param([string]$EvidenceDir = (Join-Path $PSScriptRoot ('../target/stage16/cargo-' + [guid]::NewGuid().ToString('N'))))
$ErrorActionPreference = 'Stop'
$repo16 = Split-Path $PSScriptRoot
$EvidenceDir = [IO.Path]::GetFullPath($EvidenceDir)
$testHome16 = Join-Path $EvidenceDir 'home'
New-Item -ItemType Directory -Force -Path $testHome16 | Out-Null
$cargo16 = Get-Command cargo -ErrorAction SilentlyContinue
$cargoExe16 = if ($cargo16) { $cargo16.Source } else { Join-Path $env:USERPROFILE '.cargo/bin/cargo.exe' }
function ProfileSnapshot16 {
    $snapshot16 = [ordered]@{}
    foreach ($relative16 in @('.claude.json','.claude/settings.json','.codex/config.toml','.codex/hooks.json',
        '.claude/skills/bars-docs/SKILL.md','.claude/skills/bars-session-log/SKILL.md',
        '.agents/skills/bars-docs/SKILL.md','.agents/skills/bars-session-log/SKILL.md')) {
        foreach ($suffix16 in @('', '.bars-backup')) {
            $file16 = Join-Path $env:USERPROFILE ($relative16 + $suffix16)
            $snapshot16[$relative16 + $suffix16] = if (Test-Path -LiteralPath $file16 -PathType Leaf) {
                @((Get-FileHash -LiteralPath $file16 -Algorithm SHA256).Hash, (Get-Item -LiteralPath $file16).LastWriteTimeUtc.Ticks)
            } else { $null }
        }
    }
    return ($snapshot16 | ConvertTo-Json -Depth 4 -Compress)
}
$before16 = ProfileSnapshot16
$step16 = 0
try {
    foreach ($arguments16 in @(@('test','-p','bars-core','instruction::tests'),@('test','-p','bars-worker','clients::'),@('test','-p','bars-mcp'))) {
        $step16++
        $info16 = [Diagnostics.ProcessStartInfo]::new($cargoExe16)
        $info16.UseShellExecute = $false
        $info16.CreateNoWindow = $true
        $info16.WorkingDirectory = $repo16
        $info16.RedirectStandardOutput = $true
        $info16.RedirectStandardError = $true
        $info16.Environment['BARS_USER_HOME'] = $testHome16
        $info16.Environment['CODEX_HOME'] = Join-Path $testHome16 '.codex'
        foreach ($argument16 in $arguments16) { $info16.ArgumentList.Add($argument16) }
        $process16 = [Diagnostics.Process]::Start($info16)
        try {
            $out16 = $process16.StandardOutput.ReadToEndAsync()
            $err16 = $process16.StandardError.ReadToEndAsync()
            $process16.WaitForExit()
            $out16.GetAwaiter().GetResult() | Set-Content (Join-Path $EvidenceDir "$step16.stdout.txt")
            $err16.GetAwaiter().GetResult() | Set-Content (Join-Path $EvidenceDir "$step16.stderr.txt")
            if ($process16.ExitCode) { throw "Cargo step $step16 failed; see $EvidenceDir" }
        } finally { $process16.Dispose() }
        Write-Output "PASS cargo $($arguments16 -join ' ')"
    }
} finally {
    $unchanged16 = (ProfileSnapshot16) -ceq $before16
    @{ profile_unchanged=$unchanged16; cargo_steps=$step16 } | ConvertTo-Json | Set-Content (Join-Path $EvidenceDir 'profile-guard.json')
    if (-not $unchanged16) { throw 'Real client profile changed during tests; inspect concurrent activity before continuing' }
}
