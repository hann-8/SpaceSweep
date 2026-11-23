[CmdletBinding()]
param(
    [Parameter(Mandatory)][string]$Winapp2Ini,
    [Parameter(Mandatory)][ValidatePattern('^[0-9a-f]{40}$')][string]$SourceCommit
)

$ErrorActionPreference = 'Stop'
$anchors = @{
    LocalAppData = 'local'
    AppData = 'roaming'
    CommonAppData = 'program_data'
    ProgramData = 'program_data'
    ProgramFiles = 'program_files'
    'ProgramFiles(x86)' = 'program_files_x86'
    WinDir = 'windows'
}
$purposes = @{
    Cache = 'cache'
    Caches = 'cache'
    CacheFiles = 'cache'
    'Cache Files' = 'cache'
    WebCache = 'cache'
    'Disk Cache' = 'cache'
    'Media Cache' = 'cache'
    'Media Cache Files' = 'cache'
    Log = 'logs'
    Logs = 'logs'
    CrashReports = 'crash_reports'
    'Crash Reports' = 'crash_reports'
    CrashDumps = 'crash_reports'
    Dumps = 'crash_reports'
}
$probes = [Collections.Generic.Dictionary[string,string]]::new([StringComparer]::OrdinalIgnoreCase)
$source = Get-Content -Raw -Encoding UTF8 -LiteralPath $Winapp2Ini
foreach ($entry in ($source -split '(?m)(?=^\[)')) {
    if ($entry -notmatch '^\[([^\]\r\n]+)\]' -or $entry -match '(?m)^ExcludeKey\d*=') {
        continue
    }
    $label = $Matches[1] -replace '\s+\*$', ''
    if ($label -match '\b(chat|messages?|history|passwords?|privacy|userdata|backups?|savegames?)\b|\p{Cc}') {
        continue
    }
    foreach ($line in ($entry -split '\r?\n')) {
        if ($line -notmatch '^FileKey\d*=%([^%]+)%\\([^:*?%<>"|\p{Cc}]+)\|\*(?:\|(RECURSE|REMOVESELF))?$') {
            continue
        }
        $anchor = $anchors[$Matches[1]]
        $relative = $Matches[2].Replace('\', '/')
        $parts = $relative.Split('/')
        $purpose = $purposes[$parts[-1]]
        if (!$anchor -or !$purpose -or $parts.Count -lt 2 -or
            ($parts -match '^(|\.{1,2}|IndexedDB|Sessions|Cloud Saves|Backup|Package Cache|User Data Backup)$')) {
            continue
        }
        $scope = if ($Matches.ContainsKey(3)) { 'tree' } else { 'top' }
        [void]$probes.TryAdd("$anchor/$relative", (@($anchor, $purpose, $scope, $label, $relative) -join "`t"))
    }
}
if ($probes.Count -eq 0) { throw 'No supported Winapp2 paths were found.' }

$header = @(
    '# Adapted from Winapp2.ini by the Winapp2 project and its contributors.'
    "# Source https://github.com/MoscaDotTo/Winapp2/blob/$SourceCommit/Winapp2.ini"
    '# This filtered data is licensed under CC BY-SA 4.0 https://creativecommons.org/licenses/by-sa/4.0/'
    '# Selected cache, diagnostic log and crash directories with exact paths. Deletion actions have been removed.'
    '# Anchor, purpose, depth, original entry label and relative path are tab separated.'
)
$keys = [string[]]@($probes.Keys)
[Array]::Sort($keys, [StringComparer]::OrdinalIgnoreCase)
$rows = @($header; foreach ($key in $keys) { $probes[$key] })
$target = Join-Path (Split-Path -Parent $PSScriptRoot) 'rules\application-probes.tsv'
[IO.File]::WriteAllText($target, ($rows -join "`n") + "`n", [Text.UTF8Encoding]::new($false))
"$($probes.Count) application paths written to $target"
