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
$engine = Join-Path $workspace 'target\release\spacesweep.exe'
if (!(Test-Path -LiteralPath $engine -PathType Leaf)) {
    throw 'Build SpaceSweep with scripts/build.ps1 before running a session.'
}

$stepName = $Step.ToLowerInvariant()
$token = [guid]::NewGuid().ToString('N')
if ($Step -eq 'Scan') {
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

if ($Step -eq 'Review') {
    $manifest = Join-Path $SessionDir "review-$token.json"
    $reportArgs = @{
        ScanJson = Join-Path $SessionDir 'scan-output.json'
        AnalysisJson = @(Get-AnalysisSources)
        SelectionManifest = $manifest
        OutputPath = Join-Path $SessionDir "report-review-$token.html"
    }
    $report = & (Join-Path $PSScriptRoot 'report.ps1') @reportArgs
    & $engine --state-dir $SessionDir review --report $report --manifest $manifest @Arguments
    if ($LASTEXITCODE -ne 0) { throw "SpaceSweep review failed (exit $LASTEXITCODE)." }
    return
}

if ($Step -eq 'Apply' -and ($Arguments.Count -ne 1 -or $Arguments[0] -cnotmatch '^[0-9a-f]{64}$')) {
    throw 'Apply requires the ID of one confirmed plan.'
}
$command = if ($Step -eq 'Analyze') { 'scan' } else { $stepName }
$engineArgs = @('--state-dir', $SessionDir, $command) + $Arguments
$raw = & $engine @engineArgs | Out-String
if ($LASTEXITCODE -ne 0) { throw "SpaceSweep $command failed (exit $LASTEXITCODE)." }
$result = $raw | ConvertFrom-Json

$reportPath = $null
$reportError = $null
$sourcePath = $null
try {
    $filename = switch ($Step) {
        'Scan' { 'scan-output.json' }
        'Analyze' { "analysis-$token.json" }
        'Plan' { "preview-$($result.plan_id)-$token.json" }
        'Apply' { "result-$($result.plan_id)-$token.json" }
    }
    $sourcePath = Join-Path $SessionDir $filename
    [IO.File]::WriteAllText($sourcePath, $raw, [Text.UTF8Encoding]::new($false))

    $reportArgs = @{
        ScanJson = Join-Path $SessionDir 'scan-output.json'
        AnalysisJson = @(Get-AnalysisSources)
        OutputPath = Join-Path $SessionDir "report-$stepName-$token.html"
    }
    if ($Step -eq 'Plan') {
        $reportArgs.PlanJson = $sourcePath
    }
    if ($Step -eq 'Apply') {
        $reportArgs.ApplyJson = $sourcePath
        $preview = Get-ChildItem -LiteralPath $SessionDir -Filter "preview-$($result.plan_id)-*.json" -File |
            Sort-Object LastWriteTimeUtc, Name | Select-Object -Last 1
        if ($preview) { $reportArgs.PlanJson = $preview.FullName }
    }
    $reportPath = & (Join-Path $PSScriptRoot 'report.ps1') @reportArgs
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
