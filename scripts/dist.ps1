[CmdletBinding()]
param()

$ErrorActionPreference = 'Stop'
$workspace = Split-Path -Parent $PSScriptRoot
$release = & (Join-Path $PSScriptRoot 'build.ps1') -PackageInfo
if (!$release.Engine) { throw 'Cargo did not produce a SpaceSweep executable.' }

$pe = [IO.BinaryReader]::new([IO.File]::OpenRead($release.Engine))
try {
    $pe.BaseStream.Position = 0x3c
    $pe.BaseStream.Position = $pe.ReadUInt32()
    if ($pe.ReadUInt32() -ne 0x4550 -or $pe.ReadUInt16() -ne 0x8664) {
        throw 'The distribution requires a Windows x64 build.'
    }
}
finally {
    $pe.Dispose()
}

$version = & $release.Engine --version
if ($LASTEXITCODE -ne 0 -or $version -cnotmatch '^spacesweep ([0-9]+\.[0-9]+\.[0-9]+(?:[-+][0-9A-Za-z.+-]+)?)$') {
    throw 'Could not read the SpaceSweep build version.'
}
$archiveName = "SpaceSweep-$($Matches[1])-windows-x64.zip"

$files = [ordered]@{
    'bin/spacesweep.exe' = $release.Engine
}
foreach ($path in @(
    'README.md'
    '.agents/skills/spacesweep/SKILL.md'
    'scripts/spacesweep.ps1'
    'scripts/report.ps1'
    'scripts/report-selection.js'
    'docs/usage.md'
    'docs/boundaries.md'
    'docs/application-rules.md'
    'rules/application-probes.tsv'
)) {
    $files[$path] = Join-Path $workspace $path
}
foreach ($directory in $release.Dependencies) {
    $name = Split-Path -Leaf $directory
    $notices = Get-ChildItem -LiteralPath $directory -File | Where-Object {
        $_.Name -match '^(LICEN[CS]E|COPYING|NOTICE|COPYRIGHT|UNLICENSE|AUTHORS)([-_.]|$)'
    }
    foreach ($notice in $notices) {
        $files["licenses/$name/$($notice.Name)"] = $notice.FullName
    }
}
$files['licenses/rust/COPYRIGHT-library.html'] = Join-Path $release.RustDocDir 'COPYRIGHT-library.html'

# Add entries by name so the .agents directory is included on every PowerShell version.
Add-Type -AssemblyName System.IO.Compression
Add-Type -AssemblyName System.IO.Compression.FileSystem
$dist = Join-Path $workspace 'dist'
[void][IO.Directory]::CreateDirectory($dist)
$archivePath = Join-Path $dist $archiveName
$pendingPath = Join-Path $dist ([guid]::NewGuid().ToString('N') + '.zip.tmp')
try {
    $archive = [IO.Compression.ZipFile]::Open($pendingPath, [IO.Compression.ZipArchiveMode]::Create)
    try {
        foreach ($path in $files.Keys) {
            [void][IO.Compression.ZipFileExtensions]::CreateEntryFromFile(
                $archive, $files[$path], "SpaceSweep/$path", [IO.Compression.CompressionLevel]::Optimal)
        }
    }
    finally {
        $archive.Dispose()
    }

    Move-Item -LiteralPath $pendingPath -Destination $archivePath -Force
    $sha256 = (Get-FileHash -LiteralPath $archivePath -Algorithm SHA256).Hash.ToLowerInvariant()
    [IO.File]::WriteAllText("$archivePath.sha256", "$sha256  $archiveName`n", [Text.Encoding]::ASCII)
    $archivePath
}
finally {
    if (Test-Path -LiteralPath $pendingPath -PathType Leaf) {
        Remove-Item -LiteralPath $pendingPath
    }
}
