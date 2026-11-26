# Locates the Forza Horizon 6 folder and the default backup folder for install.ps1 and
# uninstall.ps1 (dot-sourced).
# Game folder order: $env:FH6_DIR, Steam's uninstall entry for FH6, the Steam libraries listed in
# libraryfolders.vdf (via FH6's app manifest), the Game Pass / Microsoft Store package,
# Steam's default library.

$Fh6AppId = '2483190'   # Forza Horizon 6 on Steam (steam_appid.txt in the game folder)
$Fh6PackageName = 'Microsoft.ForteBaseGame'   # FH6 on Game Pass / Microsoft Store (appxmanifest.xml Identity)

function Test-Fh6Dir([string]$Dir) {
    $Dir -and (Test-Path -LiteralPath (Join-Path $Dir 'forzahorizon6.exe'))
}

# Game Pass version from the package manifest in the game folder; $null for Steam.
function Get-Fh6PackageVersion([string]$GameDir) {
    if (-not $GameDir) { return $null }
    $manifest = Join-Path $GameDir 'appxmanifest.xml'
    if (-not (Test-Path -LiteralPath $manifest)) { return $null }
    $m = [regex]::Match((Get-Content -LiteralPath $manifest -Raw), '<Identity\b[^>]*\bVersion="([^"]+)"')
    if ($m.Success) { $m.Groups[1].Value } else { $null }
}

# Steam build id from FH6's app manifest next to the library's common folder; $null if the
# game folder is not in a Steam library.
function Get-Fh6SteamBuild([string]$GameDir) {
    if (-not $GameDir) { return $null }
    $steamapps = Split-Path (Split-Path $GameDir)
    if (-not $steamapps) { return $null }
    $acf = Join-Path $steamapps "appmanifest_$Fh6AppId.acf"
    if (-not (Test-Path -LiteralPath $acf)) { return $null }
    $m = [regex]::Match((Get-Content -LiteralPath $acf -Raw), '"buildid"\s+"(\d+)"')
    if ($m.Success) { $m.Groups[1].Value } else { $null }
}

$Fh6BackupRoot = "$env:USERPROFILE\fh6-backup"

# Default backup folder: $env:FH6_BACKUP_DIR, else a subfolder of %USERPROFILE%\fh6-backup per
# game build (gamepass-<version>, steam-<buildid>), so after an update the next install run
# backs up the new originals by itself. Build unknown: %USERPROFILE%\fh6-backup itself.
function Get-Fh6BackupDir([string]$GameDir) {
    if ($env:FH6_BACKUP_DIR) { return $env:FH6_BACKUP_DIR }
    $version = Get-Fh6PackageVersion $GameDir
    if ($version) { return "$Fh6BackupRoot\gamepass-$version" }
    $build = Get-Fh6SteamBuild $GameDir
    if ($build) { return "$Fh6BackupRoot\steam-$build" }
    $Fh6BackupRoot
}

function Find-Fh6Dir {
    if ($env:FH6_DIR) { return $env:FH6_DIR }

    # Steam registers every installed game as "Steam App <id>" with its folder.
    foreach ($key in "HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\Steam App $Fh6AppId",
                     "HKLM:\SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall\Steam App $Fh6AppId") {
        $dir = (Get-ItemProperty -LiteralPath $key -ErrorAction SilentlyContinue).InstallLocation
        if (Test-Fh6Dir $dir) { return $dir }
    }

    # Every Steam library is listed in <Steam>\steamapps\libraryfolders.vdf; the one holding
    # FH6 has steamapps\appmanifest_<id>.acf, whose "installdir" names the game folder.
    $steam = @((Get-ItemProperty 'HKCU:\Software\Valve\Steam' -ErrorAction SilentlyContinue).SteamPath,
               (Get-ItemProperty 'HKLM:\SOFTWARE\WOW6432Node\Valve\Steam' -ErrorAction SilentlyContinue).InstallPath) |
        Where-Object { $_ } | Select-Object -First 1
    if ($steam) {
        $vdf = Join-Path ($steam -replace '/', '\') 'steamapps\libraryfolders.vdf'
        if (Test-Path -LiteralPath $vdf) {
            $libraries = [regex]::Matches((Get-Content -LiteralPath $vdf -Raw), '"path"\s+"([^"]+)"') |
                ForEach-Object { $_.Groups[1].Value.Replace('\\', '\') }
            foreach ($lib in $libraries) {
                $acf = Join-Path $lib "steamapps\appmanifest_$Fh6AppId.acf"
                if (-not (Test-Path -LiteralPath $acf)) { continue }
                $m = [regex]::Match((Get-Content -LiteralPath $acf -Raw), '"installdir"\s+"([^"]+)"')
                $name = if ($m.Success) { $m.Groups[1].Value } else { 'ForzaHorizon6' }
                $dir = Join-Path $lib "steamapps\common\$name"
                if (Test-Fh6Dir $dir) { return $dir }
            }
        }
    }

    # Game Pass: Windows knows the package's folder. InstallLocation is a junction chain
    # (Program Files\WindowsApps\<pkg> -> <drive>\WindowsApps\<pkg> -> <library>\Content);
    # follow it so the real folder is shown and used.
    $pkg = Get-AppxPackage -Name $Fh6PackageName -ErrorAction SilentlyContinue | Select-Object -First 1
    if ($pkg) {
        $dir = $pkg.InstallLocation
        for ($i = 0; $i -lt 5; $i++) {
            $target = (Get-Item -LiteralPath $dir -Force -ErrorAction SilentlyContinue).Target
            if (-not $target) { break }
            $dir = @($target)[0]   # string[] in PowerShell 5.1, string in 7
        }
        if (Test-Fh6Dir $dir) { return $dir.TrimEnd('\') }
    }

    return "${env:ProgramFiles(x86)}\Steam\steamapps\common\ForzaHorizon6"
}
