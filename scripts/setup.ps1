# AEGIS — compilation sous Windows (PowerShell). Prérequis : Rust (https://rustup.rs),
# Build Tools for Visual Studio (C++), Node.js 22 pour AEGIS Desktop.
# NON VÉRIFIÉ sur une machine Windows : le pipeline CI fait la même chose sur un runner Windows.
$ErrorActionPreference = "Stop"
Set-Location (Join-Path $PSScriptRoot "..")
if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) { Write-Host "Installez Rust depuis https://rustup.rs puis relancez."; exit 1 }
cargo build --release -p aegisd
New-Item -ItemType Directory -Force -Path config, data, desktop\src-tauri\binaries | Out-Null
if (-not (Test-Path config\aegis.toml)) { Copy-Item config\aegis.example.toml config\aegis.toml }
if (Get-Command npm -ErrorAction SilentlyContinue) {
    $triple = (rustc -vV | Select-String "host: ").ToString().Split(" ")[1]
    Copy-Item target\release\aegisd.exe "desktop\src-tauri\binaries\aegisd-$triple.exe" -Force
    Push-Location desktop; npm ci; npx tauri build; Pop-Location
    Write-Host "Installateurs : desktop\src-tauri\target\release\bundle\ (nsis, msi)"
} else {
    Write-Host "Node.js absent : seul le moteur est compilé (target\release\aegisd.exe)."
}
