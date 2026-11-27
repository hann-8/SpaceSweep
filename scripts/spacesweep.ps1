[CmdletBinding()]
param(
    [Parameter(Mandatory, Position = 0)]
    [ValidateSet('Scan', 'Analyze', 'Review', 'Plan', 'Apply')]
    [string]$Step,
    [string]$SessionDir,
    [string[]]$Arguments = @()
)

$ErrorActionPreference = 'Stop'
$workspace = Split-Path -Parent $PSScriptRoot
$engine = Join-Path $workspace 'bin\spacesweep.exe'
if (!(Test-Path -LiteralPath $engine -PathType Leaf)) {
    $engine = Join-Path $workspace 'target\release\spacesweep.exe'
    if (!(Test-Path -LiteralPath $engine -PathType Leaf)) {
        throw 'SpaceSweep is missing. Extract the complete release ZIP or build the source with scripts/build.ps1.'
    }
}

$stepName = $Step.ToLowerInvariant()
$token = [guid]::NewGuid().ToString('N')
if ($Step -eq 'Scan') {
    $defaultArgs = @('--discover', '--deep') | Where-Object { $_ -notin $Arguments }
    $Arguments = @($defaultArgs) + $Arguments
    $stateRoot = Join-Path $workspace '.spacesweep-state'
    [void][IO.Directory]::CreateDirectory($stateRoot)
    $storage = Get-Item -LiteralPath $stateRoot -Force
    if ($storage.Attributes -band [IO.FileAttributes]::ReparsePoint) {
        throw 'The report state root must be an ordinary directory.'
    }
    $SessionDir = Join-Path $stateRoot "session-$token"
    [void][IO.Directory]::CreateDirectory($SessionDir)
}
else {
    if (!$SessionDir) { throw "Use -SessionDir from the scan for $Step." }
    $SessionDir = (Resolve-Path -LiteralPath $SessionDir).Path
    if (!(Test-Path -LiteralPath (Join-Path $SessionDir 'scan-output.json') -PathType Leaf)) {
        throw 'The session is missing scan-output.json.'
    }
}

function Get-AnalysisSources {
    Get-ChildItem -LiteralPath $SessionDir -Filter 'analysis-*.json' -File |
        Sort-Object LastWriteTimeUtc, Name |
        Select-Object -ExpandProperty FullName
}

function Invoke-Engine([string[]]$EngineArgs) {
    $previousEncoding = [Console]::OutputEncoding
    try {
        [Console]::OutputEncoding = [Text.UTF8Encoding]::new($false)
        & $engine @EngineArgs
        if ($LASTEXITCODE -ne 0) { throw "SpaceSweep $Step failed (exit $LASTEXITCODE)." }
    }
    finally {
        [Console]::OutputEncoding = $previousEncoding
    }
}

if ($Step -eq 'Review') {
    $manifest = Join-Path $SessionDir "review-$token.json"
    $reportArgs = @{
        ScanJson = Join-Path $SessionDir 'scan-output.json'
        AnalysisJson = @(Get-AnalysisSources)
        SelectionManifest = $manifest
        OutputPath = Join-Path $SessionDir "report-review-$token.html"
    }
    $report = & (Join-Path $PSScriptRoot 'report.ps1') @reportArgs
    Invoke-Engine (@('--state-dir', $SessionDir, 'review', '--report', $report, '--manifest', $manifest) + $Arguments)
    return
}

if ($Step -eq 'Apply' -and ($Arguments.Count -ne 1 -or $Arguments[0] -cnotmatch '^[0-9a-f]{64}$')) {
    throw 'Apply requires the ID of one confirmed plan.'
}
$command = if ($Step -eq 'Analyze') { 'scan' } else { $stepName }
$engineArgs = @('--state-dir', $SessionDir, $command) + $Arguments
$raw = Invoke-Engine $engineArgs | Out-String
$result = $raw | ConvertFrom-Json

$reportPath = $null
$reportError = $null
$sourcePath = $null
try {
    $filename = switch ($Step) {
        'Scan' { 'scan-output.json' }
        'Analyze' { "analysis-$token.json" }
        'Plan' { "preview-$token.json" }
        'Apply' { "result-$token.json" }
    }
    $sourcePath = Join-Path $SessionDir $filename
    [IO.File]::WriteAllText($sourcePath, $raw, [Text.UTF8Encoding]::new($false))

    if ($Step -ne 'Apply') {
        $reportArgs = @{
            ScanJson = Join-Path $SessionDir 'scan-output.json'
            AnalysisJson = @(Get-AnalysisSources)
            OutputPath = Join-Path $SessionDir "report-$stepName-$token.html"
        }
        if ($Step -eq 'Plan') {
            $reportArgs.PlanJson = $sourcePath
        }
        $reportPath = & (Join-Path $PSScriptRoot 'report.ps1') @reportArgs
    }
}
catch {
    # Preserve the engine result, especially after an apply that has already completed.
    $reportError = $_.Exception.Message
}

$result | Add-Member -NotePropertyName session_dir -NotePropertyValue $SessionDir
$result | Add-Member -NotePropertyName source_json -NotePropertyValue $sourcePath
$result | Add-Member -NotePropertyName report_file -NotePropertyValue $reportPath
$result | Add-Member -NotePropertyName report_error -NotePropertyValue $reportError
$result | ConvertTo-Json -Depth 32
