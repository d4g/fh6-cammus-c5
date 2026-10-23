<#
.SYNOPSIS
    Builds the release zip from a built repository: the DLL, the installer, the profile and
    the documentation, plus a SHA-256 checksum file.

.DESCRIPTION
    Expects dll\target\release\xinput1_3.dll to exist (cargo build --release in dll\).
    Used by .github/workflows/release.yml; runs the same way locally.
    Layout inside the zip mirrors the repository, so install.ps1 finds the DLL next to itself
    and the profile in profile\.

.EXAMPLE
    powershell -NoProfile -ExecutionPolicy Bypass -File scripts\package.ps1 -Version v0.1.0
#>
[CmdletBinding()]
param(
    [string]$Version = 'dev',
    [string]$OutDir = 'dist'
)

$ErrorActionPreference = 'Stop'
$root = Split-Path $PSScriptRoot -Parent
$dll = Join-Path $root 'dll\target\release\xinput1_3.dll'
if (-not (Test-Path -LiteralPath $dll)) { throw "Build the DLL first: $dll is missing." }

$name = "fh6-cammus-c5-$Version"
$out = Join-Path $root $OutDir
$stage = Join-Path $out $name
if (Test-Path -LiteralPath $stage) { Remove-Item -LiteralPath $stage -Recurse -Force }
New-Item -ItemType Directory -Force -Path $stage, "$stage\profile", "$stage\docs\images" | Out-Null

Copy-Item -LiteralPath $dll -Destination $stage
foreach ($f in 'install.ps1', 'uninstall.ps1', 'README.md', 'LICENSE') { Copy-Item -LiteralPath (Join-Path $root $f) -Destination $stage }
Copy-Item -Path (Join-Path $root 'profile\*.xml') -Destination "$stage\profile"
Copy-Item -Path (Join-Path $root 'docs\*.md') -Destination "$stage\docs"
Copy-Item -Path (Join-Path $root 'docs\images\*.png') -Destination "$stage\docs\images"

$zip = Join-Path $out "$name.zip"
if (Test-Path -LiteralPath $zip) { Remove-Item -LiteralPath $zip -Force }
# Written with System.IO.Compression rather than Compress-Archive: Windows PowerShell 5.1's
# Compress-Archive stores paths with backslashes, which the zip format does not allow.
Add-Type -AssemblyName System.IO.Compression, System.IO.Compression.FileSystem
$archive = [IO.Compression.ZipFile]::Open($zip, [IO.Compression.ZipArchiveMode]::Create)
try {
    Get-ChildItem -LiteralPath $stage -Recurse -File | Sort-Object FullName | ForEach-Object {
        $entry = $_.FullName.Substring($stage.Length + 1).Replace('\', '/')
        [void][IO.Compression.ZipFileExtensions]::CreateEntryFromFile($archive, $_.FullName, $entry, [IO.Compression.CompressionLevel]::Optimal)
    }
} finally { $archive.Dispose() }
$hash = (Get-FileHash -LiteralPath $zip -Algorithm SHA256).Hash.ToLower()
# Same format as sha256sum: "<hash>  <file>"
[IO.File]::WriteAllText("$zip.sha256", "$hash  $name.zip`n")

Write-Host "zip:    $zip"
Write-Host "sha256: $hash"
