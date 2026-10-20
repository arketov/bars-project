# Compile the actual topic module without unrelated inference backends.
$ErrorActionPreference = 'Stop'
$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot '../../..')).Path.Replace('\', '/')
$harnessRoot = Join-Path ([IO.Path]::GetTempPath()) ('bars-topics-068-' + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path (Join-Path $harnessRoot 'src') | Out-Null
@'
[package]
name = "bars-topics-threshold-check"
version = "0.0.0"
edition = "2024"
[dependencies]
rusqlite = { version = "=0.40.2", features = ["bundled"] }
thiserror = "2.0"
tempfile = "3.20"
'@ | Set-Content -LiteralPath (Join-Path $harnessRoot 'Cargo.toml') -Encoding utf8
# Extract the real type declaration; no model loading and no substitute store logic.
$modelSource = Get-Content -LiteralPath "$repoRoot/crates/bars-core/src/models/embedder.rs" -Raw
$fingerprint = [regex]::Match($modelSource, 'pub struct ModelFingerprint \{[\s\S]*?\n\}').Value
if (-not $fingerprint) { throw 'ModelFingerprint declaration not found' }
@"
pub mod models { $fingerprint }
#[path="$repoRoot/crates/bars-core/src/new_topics/mod.rs"]
pub mod new_topics;
"@ | Set-Content -LiteralPath (Join-Path $harnessRoot 'src/lib.rs') -Encoding utf8
$cargo = Join-Path $env:USERPROFILE '.cargo/bin/cargo.exe'
& $cargo test --manifest-path (Join-Path $harnessRoot 'Cargo.toml') --offline -- --nocapture
exit $LASTEXITCODE
