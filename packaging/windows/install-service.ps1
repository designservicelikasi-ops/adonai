# AEGIS engine — installation as a Windows service (run in an ADMINISTRATOR PowerShell).
# Usage: .\install-service.ps1 [-Bin .\aegisd.exe] [-Dir C:\AEGIS]
param([string]$Bin = ".\aegisd.exe", [string]$Dir = "C:\AEGIS")
$ErrorActionPreference = "Stop"
if (!(Test-Path $Bin)) { throw "aegisd.exe introuvable: $Bin" }
New-Item -ItemType Directory -Force "$Dir\config" | Out-Null
Copy-Item $Bin "$Dir\aegisd.exe" -Force
$cfg = "$Dir\config\aegis.toml"
if (!(Test-Path $cfg)) {
  $example = Join-Path (Split-Path $PSScriptRoot -Parent) "config\aegis.example.toml"
  if (Test-Path $example) {
    (Get-Content $example) -replace '^data_dir = .*', 'data_dir = "C:/AEGIS/data"' | Set-Content $cfg
  } else {
    "[engine]`ndata_dir = `"C:/AEGIS/data`"" | Set-Content $cfg
  }
}
Set-Location $Dir
& "$Dir\aegisd.exe" --config $cfg service-install
Start-Service AegisEngine
Start-Sleep -Seconds 3
Get-Service AegisEngine
Write-Host "Appairage d'AEGIS Desktop :"
& "$Dir\aegisd.exe" --config $cfg pairing
