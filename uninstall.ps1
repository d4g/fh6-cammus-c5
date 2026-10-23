<#
.SYNOPSIS
    Undoes install.ps1: restores the backed-up game files and removes the FFB proxy DLL.

.DESCRIPTION
    Windows PowerShell 5.1. Run with FH6 closed. Files without a backup are left alone; Steam's
    "Verify integrity of game files" restores them as well. The backups stay in place.

.EXAMPLE
    powershell -NoProfile -ExecutionPolicy Bypass -File uninstall.ps1
#>
[CmdletBinding()]
param(
    [string]$GameDir,   # default: $env:FH6_DIR, else found through Steam (fh6-path.ps1)
    [string]$BackupDir = $(if ($env:FH6_BACKUP_DIR) { $env:FH6_BACKUP_DIR } else { "$env:USERPROFILE\fh6-backup" })
)

$ErrorActionPreference = 'Stop'
. "$PSScriptRoot\fh6-path.ps1"
if (-not $GameDir) { $GameDir = Find-Fh6Dir }
Write-Host "Game folder:   $GameDir"
Write-Host "Backup folder: $BackupDir"
if (Get-Process forzahorizon6 -ErrorAction SilentlyContinue) { throw 'Forza Horizon 6 is running. Close it first.' }
if (-not (Test-Path -LiteralPath "$GameDir\forzahorizon6.exe")) { throw "No forzahorizon6.exe in '$GameDir'. Pass -GameDir or set FH6_DIR." }

# Pairs of (backup, game file). The leading comma keeps the first pair a pair: @(@(a, b))
# would flatten into two loose paths.
$restore = @(, @("$BackupDir\inputmappingprofiles.zip", "$GameDir\media\inputmappingprofiles.zip"))
if (Test-Path -LiteralPath "$BackupDir\StringTables") {
    Get-ChildItem -LiteralPath "$BackupDir\StringTables" -Filter *.zip | ForEach-Object {
        $restore += , @($_.FullName, "$GameDir\media\Stripped\StringTables\$($_.Name)")
    }
}
foreach ($pair in $restore) {
    if (Test-Path -LiteralPath $pair[0]) {
        Copy-Item -LiteralPath $pair[0] -Destination $pair[1] -Force
        Write-Host "restored $($pair[1])"
    }
}

foreach ($f in @('xinput1_3.dll', 'fh6-ffb-fix.dedupe', 'fh6-ffb-fix.verbose')) {
    if (Test-Path -LiteralPath "$GameDir\$f") { Remove-Item -LiteralPath "$GameDir\$f"; Write-Host "removed $f" }
}
Write-Host 'Done. In FH6, pick another wheel layout if CammusC5CP5 was selected.'
