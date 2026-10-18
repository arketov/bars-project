<#
  Собирает установщик Windows (NSIS) для bars: bars.exe, bars-mcp.exe и bars-cli.exe в одном
  установщике. Описание: docs/features/distribution/installer.md.

  Использование:  pwsh scripts/build-installer.ps1 [-OutDir <папка>]
  Готовый установщик копируется в OutDir (по умолчанию dist-installer/, он в .gitignore).
#>
param([string]$OutDir = (Join-Path $PSScriptRoot '..\dist-installer'))

$ErrorActionPreference = 'Stop'
$root = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
$app = Join-Path $root 'crates\bars-app'

# Открытый до установки Rust терминал может ещё не знать каталог rustup.
if (-not (Get-Command rustc -CommandType Application -ErrorAction SilentlyContinue) -or
    -not (Get-Command cargo -CommandType Application -ErrorAction SilentlyContinue)) {
    $cargoHome = if ($env:CARGO_HOME) { $env:CARGO_HOME } else { Join-Path $env:USERPROFILE '.cargo' }
    $cargoBin = Join-Path $cargoHome 'bin'
    if (Test-Path -LiteralPath $cargoBin -PathType Container) {
        $env:PATH = "$cargoBin$([IO.Path]::PathSeparator)$env:PATH"
    }
}
foreach ($tool in 'rustc', 'cargo', 'node', 'npm') {
    if (-not (Get-Command $tool -ErrorAction SilentlyContinue)) {
        throw "Не найден $tool. Для сборки нужны Rust (rustup) и Node.js с npm. Установите недостающий инструмент и повторите запуск."
    }
}

Push-Location $root
try {
    $rustInfo = & rustc -vV
    if ($LASTEXITCODE) { throw 'rustc -vV не прошёл; проверьте toolchain из rust-toolchain.toml' }
    $hostMatch = $rustInfo | Select-String '^host: (.+)$' | Select-Object -First 1
    if (-not $hostMatch) { throw 'не удалось определить целевую платформу rustc' }
    $triple = $hostMatch.Matches[0].Groups[1].Value
} finally { Pop-Location }

Push-Location (Join-Path $root 'ui')
try {
    npm ci
    if ($LASTEXITCODE) { throw 'npm ci не прошёл' }
    npm run build
    if ($LASTEXITCODE) { throw 'npm run build не прошёл' }
} finally { Pop-Location }

Push-Location $root
try {
    cargo build --release -p bars-cli -p bars-mcp
    if ($LASTEXITCODE) { throw 'cargo build не прошёл' }
} finally { Pop-Location }

# Каталог сборки: CARGO_TARGET_DIR или target/ репозитория.
$target = if ($env:CARGO_TARGET_DIR) { $env:CARGO_TARGET_DIR } else { Join-Path $root 'target' }
$bin = Join-Path $app 'binaries'
New-Item -ItemType Directory -Force $bin | Out-Null
foreach ($name in 'bars-mcp', 'bars-cli') {
    # Tauri ждёт sidecar под именем <имя>-<triple>.exe и кладёт его в установщик без суффикса.
    Copy-Item (Join-Path $target "release\$name.exe") (Join-Path $bin "$name-$triple.exe") -Force
}

Push-Location $app
try {
    & (Join-Path $root 'ui/node_modules/.bin/tauri.cmd') build --config tauri.bundle.conf.json --bundles nsis
    if ($LASTEXITCODE) { throw 'tauri build не прошёл' }
} finally { Pop-Location }

New-Item -ItemType Directory -Force $OutDir | Out-Null
$setup = Get-ChildItem (Join-Path $target 'release\bundle\nsis') -Filter '*-setup.exe' |
    Sort-Object LastWriteTime -Descending | Select-Object -First 1
Copy-Item $setup.FullName $OutDir -Force
Write-Host "Готово: $(Join-Path $OutDir $setup.Name)"
