# Remove the AEGIS engine service (ADMINISTRATOR PowerShell). Data in C:\AEGIS\data is kept.
param([string]$Dir = "C:\AEGIS")
& "$Dir\aegisd.exe" service-uninstall
