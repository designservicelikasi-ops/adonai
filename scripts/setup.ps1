# AEGIS — installation sous Windows (PowerShell). NON VÉRIFIÉ : écrit sans machine Windows de test.
# Prérequis : Rust (https://rustup.rs) et les "Build Tools for Visual Studio" (charge de travail C++).
$ErrorActionPreference = "Stop"
Set-Location (Join-Path $PSScriptRoot "..")
if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) {
    Write-Host "Rust n'est pas installé : installez-le depuis https://rustup.rs puis relancez ce script."
    exit 1
}
cargo build --release -p aegis-control -p aegisd
New-Item -ItemType Directory -Force -Path config, data | Out-Null
if (-not (Test-Path config\aegis.toml)) { Copy-Item config\aegis.example.toml config\aegis.toml }
Write-Host "Terminé. Lancez : .\target\release\aegis.exe --config config\aegis.toml"
