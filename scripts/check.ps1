# Full local check for Windows: lint, test, release build, and a launch
# smoke test that renders the demo scene and saves a screenshot.
# Usage (PowerShell, from anywhere in the repo):  .\scripts\check.ps1
$ErrorActionPreference = "Stop"
Set-Location (Join-Path $PSScriptRoot "..")
$env:RUSTFLAGS = "$env:RUSTFLAGS -D warnings"

function Step($name, [scriptblock]$cmd) {
    Write-Host "== $name"
    & $cmd
    if ($LASTEXITCODE -ne 0) { throw "$name failed (exit $LASTEXITCODE)" }
}

Write-Host "== Windows $([Environment]::OSVersion.Version), $(rustc --version)"
Step "format" { cargo fmt --all -- --check }
Step "clippy" { cargo clippy --workspace --all-targets }
Step "tests"  { cargo test --workspace }
Step "build"  { cargo build --profile dist }

$shot = Join-Path $env:TEMP "valence-smoke.png"
Remove-Item $shot -ErrorAction SilentlyContinue
Write-Host "== smoke test (opens a window for ~10-30 s)"
$env:VALENCE_SCREENSHOT = $shot
$p = Start-Process -FilePath ".\target\dist\valence.exe" -ArgumentList "--demo" -Wait -PassThru
Remove-Item Env:\VALENCE_SCREENSHOT
if ($p.ExitCode -ne 0) { throw "game exited with code $($p.ExitCode)" }
if (-not (Test-Path $shot) -or (Get-Item $shot).Length -eq 0) { throw "no screenshot produced" }
Write-Host "OK: launched, rendered and exited cleanly. Screenshot: $shot"
Write-Host "Binary: target\dist\valence.exe"
