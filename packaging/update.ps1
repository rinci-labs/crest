<#
  For developers: copies the latest build (target\release\crest.exe) over the installed Crest,
  then restarts it. No administrator rights needed. Run after `cargo build --release`
  (or double-click update.cmd):

      powershell -ExecutionPolicy Bypass -File "<project folder>\packaging\update.ps1"

  First install: run `target\release\crest.exe --install`.
#>
$ErrorActionPreference = 'Stop'

$root = Split-Path -Parent $PSScriptRoot
$built = Join-Path $root 'target\release\crest.exe'
$installed = Join-Path $env:LOCALAPPDATA 'Programs\Crest\crest.exe'

if (-not (Test-Path $built)) { throw 'No build found. Run first: cargo build --release' }
if (-not (Test-Path $installed)) { throw 'Crest is not installed. Run first: target\release\crest.exe --install' }

Write-Host 'Stopping Crest...'
Get-Process crest -ErrorAction SilentlyContinue | Where-Object { $_.Path -eq $installed } | Stop-Process -Force
# Wait until Windows releases the exe file.
for ($i = 0; $i -lt 20; $i++) {
    try { Copy-Item $built $installed -Force; break }
    catch { if ($i -eq 19) { throw }; Start-Sleep -Milliseconds 250 }
}

Start-Process explorer.exe -ArgumentList "`"$installed`""
Write-Host 'Crest has been updated and is running again.'
