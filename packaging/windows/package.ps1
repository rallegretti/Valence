# Build Valence for Windows x64 and zip it:
#
#   dist\Valence-<version>-windows-x86_64.zip   (Valence\Valence.exe + licence)
#
# Run in PowerShell on Windows with the Rust MSVC toolchain and VS Build Tools
# (including the C++ ATL component, for the statically linked DXC).
#
# Usage: .\packaging\windows\package.ps1 [-SkipBuild]
param([switch]$SkipBuild)
$ErrorActionPreference = "Stop"
$Root = Resolve-Path (Join-Path $PSScriptRoot "..\..")
Set-Location $Root
$Version = (Select-String -Path Cargo.toml -Pattern '^version = "(.*)"' | Select-Object -First 1).Matches[0].Groups[1].Value

if (-not $SkipBuild) {
    cargo build --locked --profile dist
    if ($LASTEXITCODE -ne 0) { throw "cargo build failed" }
}

$Dist = Join-Path $Root "dist"
$Work = Join-Path $Root "target\windows-package"
$Folder = Join-Path $Work "Valence"
Remove-Item $Work -Recurse -Force -ErrorAction SilentlyContinue
New-Item -ItemType Directory $Folder -Force | Out-Null
New-Item -ItemType Directory $Dist -Force | Out-Null
Copy-Item "target\dist\valence.exe" (Join-Path $Folder "Valence.exe")
Copy-Item "assets\fonts\LICENSE-DejaVu.txt" $Folder

$Zip = Join-Path $Dist "Valence-$Version-windows-x86_64.zip"
Remove-Item $Zip -Force -ErrorAction SilentlyContinue
Compress-Archive -Path $Folder -DestinationPath $Zip
Write-Host "wrote $Zip"
Get-Item $Zip | Select-Object Name, Length
