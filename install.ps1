<#
.SYNOPSIS
    Installs the Cammus C5 + CP5 setup for Forza Horizon 6: the FFB proxy DLL, the mapping
    profile in FH6's Logitech G29 slot, and the profile label.

.DESCRIPTION
    Windows PowerShell 5.1, no other dependencies. Run with FH6 closed.

    On the first run every game file that gets changed is copied to the backup folder, but
    only if the game's copy is still the original. Every later run rebuilds from that copy,
    so the script can be run again at any time (e.g. after Steam or the Xbox app restored
    the files).
    uninstall.ps1 restores the backups and removes the DLL.

    Why each part is needed: README.md and docs/how-it-works.md.

.PARAMETER GameDir
    FH6 folder. Default: $env:FH6_DIR, else found through Steam or the Game Pass package
    (see fh6-path.ps1).
.PARAMETER BackupDir
    Folder for the untouched copies. Default: $env:FH6_BACKUP_DIR, else a folder per game build
    in %USERPROFILE%\fh6-backup (steam-<buildid>, gamepass-<version>; see fh6-path.ps1).
.PARAMETER Label
    Name FH6 shows for the profile, at most 12 bytes (UTF-8). Default: CammusC5CP5.
.PARAMETER Languages
    String tables to patch for the label (FH6's language codes, e.g. EN, DE). Default: every
    installed one; a table that cannot be patched is then skipped with a warning.

.EXAMPLE
    powershell -NoProfile -ExecutionPolicy Bypass -File install.ps1
#>
[CmdletBinding()]
param(
    [string]$GameDir,
    [string]$BackupDir,
    [string]$Label = 'CammusC5CP5',
    [string[]]$Languages,
    [string]$DllPath,
    [string]$ProfilePath,
    [switch]$SkipDll,
    [switch]$SkipProfile,
    [switch]$SkipLabel
)

$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.IO.Compression, System.IO.Compression.FileSystem
. "$PSScriptRoot\fh6-path.ps1"
if (-not $GameDir) { $GameDir = Find-Fh6Dir }
if (-not $BackupDir) { $BackupDir = Get-Fh6BackupDir $GameDir }
$perBuildBackup = -not $env:FH6_BACKUP_DIR -and $BackupDir -ne $Fh6BackupRoot -and $BackupDir -eq (Get-Fh6BackupDir $GameDir)

# --- constants -------------------------------------------------------------------------------
$SlotEntry = 'DefaultRawGameControllerMappingProfileLogitechG29.xml'
$G29VidPid = '0x046dc24f'
$BaseVidPid = '0x34160301'        # Cammus C5 base: must be the profile's primary device (FFB target)
$StringEntry = 'MyProfile.str'
$Latin1 = [Text.Encoding]::GetEncoding(28591)   # maps bytes 1:1 to chars, for byte-exact search
$Utf8 = New-Object Text.UTF8Encoding($false)
$OldLabel = "`0Logitech G29`0"

# Release layout: files next to this script. Repo layout: DLL built in dll\target\release.
if (-not $DllPath) {
    $DllPath = @("$PSScriptRoot\xinput1_3.dll", "$PSScriptRoot\dll\target\release\xinput1_3.dll") |
        Where-Object { Test-Path $_ } | Select-Object -First 1
}
if (-not $ProfilePath) { $ProfilePath = "$PSScriptRoot\profile\DefaultRawGameControllerMappingProfileCammusC5.xml" }

# --- helpers ---------------------------------------------------------------------------------
function Read-ZipEntry([string]$ZipPath, [string]$Name) {
    $zip = [IO.Compression.ZipFile]::OpenRead($ZipPath)
    try {
        $entry = $zip.GetEntry($Name)
        if (-not $entry) { throw "$ZipPath has no entry $Name" }
        $ms = New-Object IO.MemoryStream
        $s = $entry.Open()
        try { $s.CopyTo($ms) } finally { $s.Dispose() }
        return , $ms.ToArray()
    } finally { $zip.Dispose() }
}

# Rebuilds $Source into $Destination entry by entry, in the original order (FH6 ignores
# entries added to these zips; keeping the order keeps the replaced entry where it was).
function Write-ZipCopy([string]$Source, [string]$Destination, [string]$ReplaceName, [byte[]]$ReplaceBytes) {
    $tmp = "$Destination.new"
    $in = [IO.Compression.ZipFile]::OpenRead($Source)
    try {
        $fs = [IO.File]::Create($tmp)
        $out = New-Object IO.Compression.ZipArchive($fs, [IO.Compression.ZipArchiveMode]::Create)
        try {
            foreach ($e in $in.Entries) {
                $n = $out.CreateEntry($e.FullName, [IO.Compression.CompressionLevel]::Optimal)
                $n.LastWriteTime = $e.LastWriteTime
                $w = $n.Open()
                try {
                    if ($e.FullName -eq $ReplaceName) { $w.Write($ReplaceBytes, 0, $ReplaceBytes.Length) }
                    else { $r = $e.Open(); try { $r.CopyTo($w) } finally { $r.Dispose() } }
                } finally { $w.Dispose() }
            }
        } finally { $out.Dispose(); $fs.Dispose() }
    } finally { $in.Dispose() }
    Move-Item -LiteralPath $tmp -Destination $Destination -Force
}

# Copies a game file to the backup folder once, if it is still the original.
function Confirm-Backup([string]$GameFile, [string]$BackupFile, [scriptblock]$IsOriginal) {
    if (Test-Path -LiteralPath $BackupFile) { return }
    if (-not (& $IsOriginal $GameFile)) {
        # Installs from before the per-build folders kept their backups in fh6-backup itself;
        # a still-patched game is restored from those, as before.
        $legacy = $Fh6BackupRoot + $BackupFile.Substring($BackupDir.Length)
        if ($perBuildBackup -and (Test-Path -LiteralPath $legacy) -and (& $IsOriginal $legacy)) {
            New-Item -ItemType Directory -Force -Path (Split-Path $BackupFile) | Out-Null
            Copy-Item -LiteralPath $legacy -Destination $BackupFile
            Write-Host "  took over the earlier backup $legacy"
            return
        }
        throw "$GameFile is not the original file. Restore the game files first (Steam: Properties > Installed Files > Verify integrity of game files; Xbox app: Manage > Files > Verify and repair), then run again."
    }
    New-Item -ItemType Directory -Force -Path (Split-Path $BackupFile) | Out-Null
    Copy-Item -LiteralPath $GameFile -Destination $BackupFile
    Write-Host "  backed up $GameFile"
}

# --- checks ----------------------------------------------------------------------------------
if (Get-Process forzahorizon6 -ErrorAction SilentlyContinue) { throw 'Forza Horizon 6 is running. Close it first.' }
if (-not (Test-Path -LiteralPath "$GameDir\forzahorizon6.exe")) {
    throw "No forzahorizon6.exe in '$GameDir'. Pass -GameDir or set FH6_DIR."
}
Write-Host "Game folder:   $GameDir"
Write-Host "Backup folder: $BackupDir"

# --- 1. DLL ----------------------------------------------------------------------------------
if (-not $SkipDll) {
    Write-Host '[1/3] FFB proxy DLL'
    if (-not $DllPath -or -not (Test-Path -LiteralPath $DllPath)) { throw 'xinput1_3.dll not found. Build it (dll\) or pass -DllPath.' }
    # The Game Pass build imports xinput1_4.dll instead; the proxy serves under either name.
    $dllName = if (Get-Fh6PackageVersion $GameDir) { 'xinput1_4.dll' } else { 'xinput1_3.dll' }
    Copy-Item -LiteralPath $DllPath -Destination "$GameDir\$dllName" -Force
    Write-Host "  copied $DllPath as $dllName"
}

# --- 2. profile in the G29 slot --------------------------------------------------------------
if (-not $SkipProfile) {
    Write-Host '[2/3] mapping profile (Logitech G29 slot)'
    $gameZip = "$GameDir\media\inputmappingprofiles.zip"
    $backupZip = "$BackupDir\inputmappingprofiles.zip"
    Confirm-Backup $gameZip $backupZip {
        param($f) $Latin1.GetString((Read-ZipEntry $f $SlotEntry)).ToLower().Contains($G29VidPid)
    }

    $raw = [IO.File]::ReadAllBytes($ProfilePath)
    if ($raw.Length -ge 3 -and $raw[0] -eq 0xEF -and $raw[1] -eq 0xBB -and $raw[2] -eq 0xBF) { throw "$ProfilePath starts with a BOM; FH6 skips such files." }
    $profile = $Utf8.GetString($raw)
    $stock = $Utf8.GetString((Read-ZipEntry $backupZip $SlotEntry))
    $g29Id = [regex]::Match($stock, 'Id="([^"]+)"').Groups[1].Value
    $g29Name = [regex]::Match($stock, 'UserFacingName="([^"]+)"').Groups[1].Value

    $count = 0
    foreach ($pair in @(@('Id', $g29Id), @('UserFacingName', $g29Name), @('PrimaryDeviceVidPid', $BaseVidPid))) {
        $rx = New-Object regex ('\b' + $pair[0] + '="[^"]*"')
        if ($rx.IsMatch($profile)) { $count++ }
        $profile = $rx.Replace($profile, $pair[0] + '="' + $pair[1] + '"', 1)
    }
    if ($count -ne 3) { throw 'Profile header not as expected (Id, UserFacingName, PrimaryDeviceVidPid).' }
    $bytes = $Utf8.GetBytes($profile)

    Write-ZipCopy $backupZip $gameZip $SlotEntry $bytes
    $check = Read-ZipEntry $gameZip $SlotEntry
    if ([Convert]::ToBase64String($check) -ne [Convert]::ToBase64String($bytes)) { throw 'Read-back of the profile entry differs.' }
    Write-Host "  profile written ($($bytes.Length) bytes, id $g29Id)"
}

# --- 3. label in the string tables -----------------------------------------------------------
if (-not $SkipLabel) {
    Write-Host "[3/3] profile label '$Label'"
    $new = $Utf8.GetBytes($Label)
    if ($new.Length -lt 1 -or $new.Length -gt 12) { throw "Label must be 1-12 UTF-8 bytes; '$Label' has $($new.Length)." }
    $replacement = "`0" + $Latin1.GetString($new) + ("`0" * (13 - $new.Length))   # same length as "`0Logitech G29`0"

    # Default: every installed language. Game Pass keeps 0-byte placeholders for language
    # packs that are not downloaded. A table named with -Languages must patch; one found
    # here is skipped with a warning if it does not.
    $strict = [bool]$Languages
    if (-not $strict) {
        $Languages = Get-ChildItem -LiteralPath "$GameDir\media\Stripped\StringTables" -Filter *.zip |
            Where-Object { $_.Length -gt 0 } | ForEach-Object { $_.BaseName }
    }
    foreach ($lang in $Languages) {
        $gameTable = "$GameDir\media\Stripped\StringTables\$lang.zip"
        $backupTable = "$BackupDir\StringTables\$lang.zip"
        if (-not (Test-Path -LiteralPath $gameTable)) { Write-Warning "no string table $lang; skipped"; continue }
        if ((Get-Item -LiteralPath $gameTable).Length -eq 0) { Write-Warning "string table $lang not installed (empty file); skipped"; continue }
        try {
            Confirm-Backup $gameTable $backupTable {
                param($f) $Latin1.GetString((Read-ZipEntry $f $StringEntry)).Contains($OldLabel)
            }
            $text = $Latin1.GetString((Read-ZipEntry $backupTable $StringEntry))
            $hits = ([regex]::Matches($text, [regex]::Escape($OldLabel))).Count
            if ($hits -ne 1) { throw "$lang : expected one 'Logitech G29' text, found $hits." }
            $patched = $Latin1.GetBytes($text.Replace($OldLabel, $replacement))
            Write-ZipCopy $backupTable $gameTable $StringEntry $patched
            Write-Host "  $lang patched"
        } catch {
            if ($strict) { throw }
            Write-Warning "$lang skipped: $($_.Exception.Message)"
        }
    }
}

Write-Host ''
Write-Host 'Done. In FH6: Settings > Advanced Controls > Invert Force Feedback = On;'
Write-Host "Controls > Wheel > Layout = $Label; for telemetry: Settings > HUD and Gameplay > Data Out = On, 127.0.0.1, port 20779."
Write-Host ''
$verify = if (Get-Fh6PackageVersion $GameDir) { 'the Xbox app''s "Verify and repair"' } else { 'Steam''s "Verify integrity of game files"' }
Write-Host "Run this script again after every FH6 update and after ${verify}:"
Write-Host 'both restore the game files and remove the profile and the label (the DLL stays).'
if ($perBuildBackup) {
    Write-Host 'Backups are kept per game build, so nothing needs deleting after an update.'
} else {
    Write-Host "After an FH6 update, first delete the old copies in $BackupDir (they belong to the previous version)."
}
