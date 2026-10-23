# Locates the Forza Horizon 6 folder for install.ps1 and uninstall.ps1 (dot-sourced).
# Order: $env:FH6_DIR, Steam's uninstall entry for FH6, the Steam libraries listed in
# libraryfolders.vdf (via FH6's app manifest), Steam's default library.

$Fh6AppId = '2483190'   # Forza Horizon 6 on Steam (steam_appid.txt in the game folder)

function Test-Fh6Dir([string]$Dir) {
    $Dir -and (Test-Path -LiteralPath (Join-Path $Dir 'forzahorizon6.exe'))
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

    return "${env:ProgramFiles(x86)}\Steam\steamapps\common\ForzaHorizon6"
}
