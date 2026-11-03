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
# Exit codes: 0 = drew the game, 3 = frame was blank (locked/asleep screen),
# 4 = no screenshot captured.
switch ($p.ExitCode) {
    0 { Write-Host "OK: launched, rendered the game and exited cleanly. Screenshot: $shot" }
    3 { throw "the game ran but the frame was blank. Unlock the screen / wake the display and retry." }
    default { throw "app exited with code $($p.ExitCode)" }
}
$ft = Join-Path $env:TEMP "valence-feature-test"
Remove-Item $ft -Recurse -Force -ErrorAction SilentlyContinue
New-Item -ItemType Directory $ft | Out-Null
Write-Host "== feature test (reactions, exports, settings, lab data save/open, library; opens a window)"
$env:VALENCE_FEATURE_TEST = $ft; $env:VALENCE_EXPORT_DIR = $ft
$p = Start-Process -FilePath ".\target\dist\valence.exe" -Wait -PassThru
Remove-Item Env:\VALENCE_FEATURE_TEST, Env:\VALENCE_EXPORT_DIR
$exports = @(Get-ChildItem (Join-Path $ft "valence-*.png") -ErrorAction SilentlyContinue).Count
if ($p.ExitCode -ne 0 -or $exports -ne 3) { throw "feature test failed (exit $($p.ExitCode), $exports exports) - see $ft" }
Write-Host "OK: exports, lab data round trip and library shot written to $ft"
Write-Host "Binary: target\dist\valence.exe"
