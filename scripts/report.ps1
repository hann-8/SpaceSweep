[CmdletBinding()]
param(
    [Parameter(Mandatory)][string]$ScanJson,
    [string[]]$AnalysisJson = @(),
    [string]$PlanJson,
    [string]$ApplyJson,
    [Parameter(Mandatory)][string]$OutputPath
)

$ErrorActionPreference = 'Stop'
$html = [Text.StringBuilder]::new()

function Read-ReportData([string]$File) {
    Get-Content -LiteralPath $File -Raw -Encoding UTF8 | ConvertFrom-Json
}

function Escape-Html($Value) {
    [Net.WebUtility]::HtmlEncode([string]$Value)
}

function Add-Section([string]$Content) {
    [void]$html.AppendLine($Content)
}

function Format-Bytes($Bytes) {
    if ($null -eq $Bytes) { return '未知' }
    $number = [double]$Bytes
    $size = [Math]::Abs($number)
    if ($size -ge 1TB) { return '{0:N2} TiB' -f ($number / 1TB) }
    if ($size -ge 1GB) { return '{0:N1} GiB' -f ($number / 1GB) }
    if ($size -ge 1MB) { return '{0:N1} MiB' -f ($number / 1MB) }
    if ($size -ge 1KB) { return '{0:N1} KiB' -f ($number / 1KB) }
    '{0:N0} B' -f $number
}

function Format-Count($Value) {
    '{0:N0}' -f [long]$Value
}

function Get-Percent($Part, $Whole) {
    if ($Whole -le 0) { return 0 }
    $fraction = [Math]::Max(0.0, [Math]::Min(1.0, [double]$Part / [double]$Whole))
    [int][Math]::Round($fraction * 100)
}

function Get-ReasonLabel([string]$Code) {
    switch ($Code) {
        'access_denied' { '权限不足' }
        'cancelled' { '用户取消' }
        'chrome_running' { 'Chrome 正在运行' }
        'chrome_cache_layout_not_verified' { '缓存布局待验证' }
        'chrome_version_not_verified' { 'Chrome 版本待验证' }
        'reparse_or_offline' { '链接、挂载点或离线占位' }
        'time_limit' { '达到扫描时限' }
        'missing_or_changed' { '扫描时消失或变动' }
        'read_error' { '读取错误' }
        default { $Code }
    }
}

$scan = Read-ReportData $ScanJson
if ($null -eq $scan.schema_version -or $null -eq $scan.volumes) {
    throw 'ScanJson must be the JSON output of the SpaceSweep scan command.'
}
$plan = if ($PlanJson) { Read-ReportData $PlanJson }
$applied = if ($ApplyJson) { Read-ReportData $ApplyJson }
if ($plan -and $applied -and $plan.plan_id -ne $applied.plan_id) {
    throw 'PlanJson and ApplyJson refer to different plans.'
}
$analyses = @(
    $scan
    foreach ($inputPath in $AnalysisJson) { Read-ReportData $inputPath }
)
$areas = @($analyses | ForEach-Object { $_.analysis } | Where-Object { $_ })
$discoveries = @($analyses | ForEach-Object { $_.discovery } | Where-Object { $_ })

function Get-CoverageLabel($Volume) {
    $roots = @($Volume.mounts | ForEach-Object { $_.TrimEnd([char[]]@('\', '/')) })
    $areasOnVolume = @($areas | Where-Object volume_id -eq $Volume.id)
    $whole = $areasOnVolume | Where-Object {
        $_.root -and ($roots -contains $_.root.TrimEnd([char[]]@('\', '/')))
    } | Select-Object -Last 1
    if ($whole) {
        if ($whole.error) { return '整卷分析无法读取' }
        if ($whole.stop_reason) {
            return '整卷分析部分覆盖 · ' + (Get-ReasonLabel $whole.stop_reason)
        }
        if ($whole.status -eq 'partial') { return '已遍历可访问目录 · 有跳过项' }
        return '整卷目录已遍历'
    }
    if ($areasOnVolume.Count) { return '已分析选定目录' }
    if ($scan.mode -eq 'directory_analysis' -and $scan.stop_reason) { return '目录分析未开始' }
    '快查 · 已知位置'
}

Add-Section @'
<!doctype html>
<html lang="zh-CN">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>SpaceSweep · 磁盘空间报告</title>
<style>
:root { color-scheme: light; --ink:#142b32; --muted:#456168; --line:#d6e5e6; --teal:#11766b; --teal-light:#d6efec; --paper:#fff; --back:#f0f5f5; }
* { box-sizing:border-box; }
body { margin:0; background:var(--back); color:var(--ink); font:15px/1.55 "Segoe UI","Microsoft YaHei",system-ui,sans-serif; }
main { width:min(1160px,100%); margin:auto; padding:38px 26px 52px; }
header { padding-bottom:18px; border-bottom:1px solid var(--line); }
h1,h2,h3,p { margin:0; }
h1 { font-size:clamp(24px,4vw,36px); line-height:1.25; }
h2 { font-size:21px; margin-bottom:9px; }
h3 { font-size:17px; }
p { margin:5px 0 10px; }
.muted { color:var(--muted); }
nav { display:flex; flex-wrap:wrap; gap:16px; margin-top:13px; }
a { color:#075b52; text-underline-offset:3px; }
section { margin-top:24px; }
.grid { display:grid; grid-template-columns:repeat(auto-fit,minmax(210px,1fr)); gap:12px; }
.card,.panel,details { background:var(--paper); border:1px solid var(--line); border-radius:12px; }
.card,.panel { padding:14px 17px; }
.card .value { font-size:25px; font-weight:650; display:block; margin:3px 0 8px; }
.coverage { margin-top:8px; border-top:1px solid var(--line); padding-top:7px; color:var(--muted); }
.numbers,.bytes { font-variant-numeric:tabular-nums; }
.meter { height:9px; border-radius:9px; background:var(--teal-light); overflow:hidden; }
.meter span { display:block; height:100%; border-radius:9px; background:var(--teal); }
.status { color:#654316; background:#fff2db; border-radius:7px; padding:2px 7px; font-size:13px; }
.status-complete { color:#075b52; background:var(--teal-light); }
.summary { font-size:21px; font-weight:650; }
.table-wrap { overflow-x:auto; }
table { border-collapse:collapse; width:100%; text-align:left; }
th,td { border-bottom:1px solid var(--line); padding:9px 8px; vertical-align:top; }
th { color:var(--muted); font-weight:600; }
th:first-child,td:first-child { padding-left:0; }
tr:last-child td { border-bottom:0; }
.bytes { white-space:nowrap; }
.path { overflow-wrap:anywhere; font-family:inherit; }
.rank { margin:8px 0 3px; }
.rank-label { display:flex; justify-content:space-between; align-items:baseline; gap:12px; margin-bottom:4px; }
details { padding:11px 15px; margin:8px 0; }
details details { background:#f9fbfb; }
summary { cursor:pointer; font-weight:650; }
.files { margin:8px 0; padding-left:22px; }
small { font-size:13px; }
footer { color:var(--muted); border-top:1px solid var(--line); margin-top:26px; padding-top:12px; }
@media (max-width:580px) { main { padding:22px 12px 32px; } .grid { grid-template-columns:1fr; } table { min-width:480px; } }
</style>
</head>
<body><main>
<header>
<h1>SpaceSweep 磁盘空间报告</h1>
<nav aria-label="报告导航">
'@

if ($discoveries.Count) { Add-Section '<a href="#junk">垃圾发现</a>' }
Add-Section '<a href="#volumes">磁盘容量</a><a href="#preview">清理预览</a><a href="#directories">目录排行</a><a href="#known">已知位置</a></nav></header>'

if ($discoveries.Count) {
    $findings = @($discoveries.findings | Group-Object -Property kind,path | ForEach-Object { $_.Group[-1] })
    $identified = @($findings | Where-Object identified)
    $clues = @($findings | Where-Object { !$_.identified })
    $total = Format-Bytes (($identified | Measure-Object bytes -Sum).Sum)
    $count = Format-Count (($identified | Measure-Object files -Sum).Sum)
    $elapsed = Format-Count ($analyses[-1].elapsed_ms)
    Add-Section "<section id='junk'><h2>垃圾发现</h2><p class='summary'>$count 项 · $total</p><p class='muted'>已识别项目的文件大小与回收站用量 · 最近一次扫描 $elapsed ms</p>"
    foreach ($collection in @(@{ Label = '已识别的垃圾'; Rows = $identified; Open = ' open' }, @{ Label = '待核实的残留线索'; Rows = $clues; Open = '' })) {
        $groups = Format-Count $collection.Rows.Count
        Add-Section "<details$($collection.Open)><summary>$($collection.Label) · $groups 组</summary>"
        if ($collection.Rows.Count) {
            Add-Section '<div class="table-wrap"><table><thead><tr><th>类别与位置</th><th>大小</th><th>条目</th><th>发现依据</th></tr></thead><tbody>'
            foreach ($finding in ($collection.Rows | Sort-Object bytes -Descending)) {
                $label = Escape-Html $finding.label
                $location = Escape-Html $finding.path
                $size = Format-Bytes $finding.bytes
                $items = Format-Count $finding.files
                $evidence = Escape-Html $finding.evidence
                $examples = Escape-Html ($finding.examples -join ' · ')
                Add-Section "<tr><td>$label<br><small class='muted path'>$location</small></td><td class='bytes'>$size</td><td class='bytes'>$items</td><td>$evidence<br><small class='muted path'>$examples</small></td></tr>"
            }
            Add-Section '</tbody></table></div>'
        }
        Add-Section '</details>'
    }
    $checks = @($discoveries.checks | Where-Object { $_ })
    if ($checks.Count) {
        Add-Section '<details><summary>垃圾快查范围</summary><ul class="files">'
        foreach ($check in $checks) {
            $label = Escape-Html $check.label
            $location = Escape-Html $check.path
            $status = switch ($check.status) {
                'checked' { '已查' }
                'not_found' { '位置不存在' }
                'partial' { '部分覆盖' }
                default { '无法查到' }
            }
            $detail = Escape-Html $check.detail
            Add-Section "<li>$label · <span class='path'>$location</span> · $status $detail</li>"
        }
        Add-Section '</ul></details>'
    }
    Add-Section '</section>'
}

Add-Section '<section id="volumes"><h2>磁盘容量</h2><div class="grid">'
foreach ($volume in $scan.volumes) {
    $mount = [string]$volume.mounts[0]
    $name = Escape-Html $mount
    $filesystem = Escape-Html $volume.filesystem
    if ($null -eq $volume.space) {
        $reason = Escape-Html $volume.space_error
        Add-Section "<article class='card'><h3>$name</h3><p>容量无法读取</p><small>$reason</small></article>"
        continue
    }
    $used = [Math]::Max(0.0, [double]$volume.space.total_bytes - [double]$volume.space.available_bytes)
    $usedPercent = Get-Percent $used $volume.space.total_bytes
    $free = Format-Bytes $volume.space.available_bytes
    $total = Format-Bytes $volume.space.total_bytes
    $occupied = Format-Bytes $used
    $coverage = Escape-Html (Get-CoverageLabel $volume)
    Add-Section @"
<article class="card">
<h3>$name <small class="muted">$filesystem</small></h3>
<span class="value numbers">可用 $free</span>
<div class="meter" role="img" aria-label="已用 $usedPercent%"><span style="width:$usedPercent%"></span></div>
<p class="muted numbers">已用 $occupied · 总容量 $total</p>
<div class="coverage"><small>$coverage</small></div>
</article>
"@
}
Add-Section '</div></section>'

Add-Section '<section id="preview"><h2>清理预览</h2><div class="panel">'
if ($plan) {
    $estimate = ($plan.groups | Measure-Object -Property allocated_bytes -Sum).Sum
    $count = Format-Count $plan.files
    $saved = Format-Bytes ([double]$estimate)
    $stage = if ($applied) { '执行前的估算。' } else { '本报告为只读预览。实际执行需要确认计划。' }
    Add-Section "<p class='summary'>$count 项，预计 $saved</p><p class='muted'>$stage</p>"
    if ($plan.groups) {
        Add-Section '<div class="table-wrap"><table><thead><tr><th>位置</th><th>项目</th><th>数量</th><th>预计占用</th></tr></thead><tbody>'
        foreach ($group in $plan.groups) {
            $name = Escape-Html $group.volume
            $label = Escape-Html $group.label
            $files = Format-Count $group.files
            $bytes = Format-Bytes $group.allocated_bytes
            Add-Section "<tr><td>$name</td><td>$label</td><td class='bytes'>$files</td><td class='bytes'>$bytes</td></tr>"
        }
        Add-Section '</tbody></table></div>'
    }
    if ($plan.action -eq 'permanent_delete' -and $plan.files -gt 0) {
        Add-Section '<p><small>此计划使用永久删除。完整条目以本机计划文件为准。</small></p>'
    }
}
elseif ($applied) {
    Add-Section '<p>本次处理见下方清理结果。</p>'
}
else {
    Add-Section '<p>本报告仅包含扫描数据。已知位置会标出符合预览条件的数量。</p>'
}
Add-Section '</div></section>'

if ($applied) {
    Add-Section '<section id="result"><h2>清理结果</h2><div class="panel">'
    $removed = Format-Count $applied.deleted_files
    $size = Format-Bytes $applied.deleted_logical_bytes
    $skipped = Format-Count $applied.skipped_files
    $failed = Format-Count $applied.failed_files
    $pending = Format-Count $applied.submitted_files
    $unattempted = Format-Count $applied.unattempted_files
    Add-Section "<p class='summary'>已删除 $removed 项 · 文件大小 $size</p>"
    Add-Section "<p>跳过 $skipped · 失败 $failed · 待释放 $pending · 未处理 $unattempted</p>"
    Add-Section '<div class="table-wrap"><table><thead><tr><th>磁盘</th><th>执行前可用</th><th>执行后可用</th><th>可用空间净变化</th></tr></thead><tbody>'
    foreach ($volume in $applied.volumes) {
        $name = Escape-Html $volume.mount
        $before = Format-Bytes $volume.available_before
        $after = Format-Bytes $volume.available_after
        $delta = Format-Bytes $volume.available_change_bytes
        Add-Section "<tr><td>$name</td><td>$before</td><td>$after</td><td>$delta</td></tr>"
    }
    Add-Section '</tbody></table></div><small class="muted">卷的净变化也会反映同期的系统活动。</small></div></section>'
}

Add-Section '<section id="directories"><h2>目录占用排行</h2><p class="muted">按已读取文件的逻辑大小汇总。目录大小反映占用，清理资格以预览为准。</p>'
foreach ($item in $areas) {
    $root = Escape-Html $item.root
    $status = if ($item.error) { '无法读取' } elseif ($item.status -eq 'partial') { '部分覆盖' } else { '已分析' }
    $partial = if ($item.stop_reason) { ' · ' + (Escape-Html (Get-ReasonLabel $item.stop_reason)) }
    $badge = if ($status -eq '已分析') { 'status status-complete' } else { 'status' }
    Add-Section "<details open><summary><span class='path'>$root</span> <span class='$badge'>$status$partial</span></summary>"
    if ($item.error) {
        $reason = Escape-Html $item.error
        Add-Section "<p>$reason</p></details>"
        continue
    }
    $bytes = Format-Bytes $item.totals.logical_bytes
    $files = Format-Count $item.totals.files
    $dirs = Format-Count $item.totals.directories
    Add-Section "<p class='muted'>已读 $files 个文件、$dirs 个目录，文件大小合计 $bytes。</p>"
    $max = ($item.largest_directories | Measure-Object -Property logical_bytes -Maximum).Maximum
    foreach ($dir in $item.largest_directories) {
        $path = Escape-Html $dir.path
        $size = Format-Bytes $dir.logical_bytes
        $width = Get-Percent $dir.logical_bytes $max
        Add-Section "<div class='rank'><div class='rank-label'><span class='path'>$path</span><b class='bytes'>$size</b></div><div class='meter' aria-hidden='true'><span style='width:$width%'></span></div></div>"
    }
    if ($item.totals.skipped) {
        $reasons = foreach ($entry in $item.totals.skipped.PSObject.Properties) {
            '{0} {1}' -f (Get-ReasonLabel $entry.Name), (Format-Count $entry.Value)
        }
        if ($reasons) {
            $reason = Escape-Html ($reasons -join ' · ')
            Add-Section "<p><small>跳过 $reason</small></p>"
        }
    }
    if ($item.largest_files) {
        Add-Section '<details><summary>查看最大文件</summary><ul class="files">'
        foreach ($file in $item.largest_files) {
            $path = Escape-Html $file.path
            $size = Format-Bytes $file.logical_bytes
            Add-Section "<li><span class='path'>$path</span> · <span class='bytes'>$size</span></li>"
        }
        Add-Section '</ul></details>'
    }
    Add-Section '</details>'
}
if (!$areas.Count) {
    Add-Section '<p>这轮只检查了已知位置。目录分析结果可以附在这份报告里。</p>'
}
Add-Section '</section>'

Add-Section '<section id="known"><h2>已知位置检查</h2><div class="panel table-wrap"><table><thead><tr><th>项目与位置</th><th>已读文件大小</th><th>命中文件大小</th><th>当前处理条件</th></tr></thead><tbody>'
foreach ($rule in $scan.rules | Where-Object status -ne 'not_found') {
    $label = Escape-Html $rule.label
    $root = Escape-Html $rule.root
    $read = Format-Bytes $rule.totals.logical_bytes
    $bytes = Format-Bytes $rule.matching_logical_bytes
    $action = if ($rule.status -eq 'unavailable') { '无法读取' }
        elseif ($rule.blocked_by) { Get-ReasonLabel $rule.blocked_by }
        elseif ($rule.eligible_files -gt 0) { '可进预览 ' + (Format-Count $rule.eligible_files) + ' 项' }
        else { '仅供查看' }
    $safeAction = Escape-Html $action
    Add-Section "<tr><td>$label<br><small class='muted path'>$root</small></td><td class='bytes'>$read</td><td class='bytes'>$bytes</td><td>$safeAction</td></tr>"
}
Add-Section '</tbody></table></div></section>'
Add-Section '<footer>磁盘容量、目录统计和预览量采用各自口径。报告只使用本机扫描记录。</footer></main></body></html>'

$fullPath = [IO.Path]::GetFullPath($OutputPath)
$stream = [IO.File]::Open($fullPath, [IO.FileMode]::CreateNew, [IO.FileAccess]::Write)
try {
    $bytes = [Text.Encoding]::UTF8.GetBytes($html.ToString())
    $stream.Write($bytes, 0, $bytes.Length)
}
finally {
    $stream.Dispose()
}
$fullPath
