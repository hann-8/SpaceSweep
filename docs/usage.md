# SpaceSweep 使用说明

## 在项目里使用 skill

在 Windows 上，从 SpaceSweep 项目会话发起「用 SpaceSweep 检查所有盘」或「用 SpaceSweep 把能清的都列出来」。也可以显式选 `$spacesweep`。仓库内的入口是 [SpaceSweep skill](../.agents/skills/spacesweep/SKILL.md)。 [OpenAI Docs 的项目 skill 位置](https://developers.openai.com/api/reference/cli)

已构建的程序在 `target/release/spacesweep.exe`。重新构建时，在仓库根运行 `.\scripts\build.ps1`。本脚本使用当前机器已有的 Cargo，或使用已经放进 `.spacesweep-dev` 的本地工具链。Windows GNU 工具链需能找到其 `dlltool` 和汇编器。 [Rust 的 dlltool 说明](https://doc.rust-lang.org/rustc/codegen-options/index.html#dlltool)

## 命令行

从仓库根执行。所有命令的结果是 JSON，`--progress` 的进度走标准错误。

```powershell
.\target\release\spacesweep.exe scan --discover
.\target\release\spacesweep.exe scan --discover --deep --progress
.\target\release\spacesweep.exe scan --progress
.\target\release\spacesweep.exe scan --volume D:
.\target\release\spacesweep.exe scan --path 'D:\待分析目录' --top 10
.\target\release\spacesweep.exe scan --volume E: --deep --max-seconds 20
```

`--discover` 查各盘回收站、常用缓存和临时产物，输出 `discovery.findings`。每组包含盘、路径、类别、条目数、大小、依据和 `review_level`。加 `--deep` 会在逐盘枚举时识别更多散落的转储和临时产物，以及使用 Chromium、Firefox、npm 通用布局的缓存。指定目录分析也会识别其范围内的这些缓存。 `discovery.quick_elapsed_ms` 记录完成已知来源用时，`elapsed_ms` 记录本次总耗时。

快查会列出已挂载本地盘，并检查支持的路径。目录分析按文件属性统计。使用 `--max-seconds` 可限制一轮扫描。到达时限时输出已经走到的范围，`stop_reason` 为 `time_limit`。目录排行按已读文件的逻辑大小汇总，有未读范围或跳过项时显示部分覆盖。实际删除试点覆盖当前用户的旧临时文件和已验证的 Chrome 网页磁盘缓存。 [已覆盖的规则](application-rules.md)

有可预览项时，扫描返回 `scan_id`。将它填入预览命令。预览可按规则、盘或准确路径排除，也可重复传入多个 `--scan`。

```powershell
.\target\release\spacesweep.exe plan --scan '<scan_id>' --exclude-rule chrome.http_cache
```

检查预览的分组和 `plan_file` 明细。想执行这份明确的永久删除计划时，在标准用户终端使用返回的 `plan_id`。

```powershell
.\target\release\spacesweep.exe apply '<plan_id>'
```

执行结果给出成功、跳过、失败、待释放和各盘净变化。默认状态与明细在当前用户 LocalAppData 下的 `SpaceSweep`。Windows 把默认目录重定向到应用的 LocalCache 时，返回解析后的实际位置。亦可用 `--state-dir <现有本机目录>`，整个流程使用同一目录。 [Windows 应用数据重定向](https://learn.microsoft.com/en-us/windows/msix/desktop/desktop-to-uwp-behind-the-scenes#file-system)

## 阅读结果

- **红色 · 确认可删** (`confirmed`) 表示符合已验证的旧临时文件规则，文件当前状态通过复核，清理对正常使用无已知影响。
- **黄色 · 需要审核** (`needs_review`) 表示文件用途已识别，清理前要确认缓存重建代价、回收站恢复需求、诊断需求或当前状态。
- **绿色 · 疑似垃圾** (`suspected`) 表示有残留线索，需进一步确定来源或用途。
- 扫描的 `matching_logical_bytes` 是匹配规则的文件长度。通过当前执行前置条件的数量为 `eligible_files`。Chrome 正在运行时仍可看到占用和阻塞原因。
- 预览的 `allocated_bytes` 用于估算影响。系统条目、个人资料和待应用内处理项有各自的边界。
- 执行后优先看 `deleted_files`、`outcomes`、各盘的 `available_change_bytes` 和本机 `receipt_file`。卷净变化包括同期的系统活动。 [卷容量接口](https://learn.microsoft.com/en-us/windows/win32/api/fileapi/nf-fileapi-getdiskfreespaceexw)

## 本地可视化报告

项目 skill 通过 `scripts/spacesweep.ps1` 运行，各阶段都返回 `report_file`。用首轮返回的 `session_dir` 继续本轮检查。报告首屏显示红、黄、绿各级的条目数和大小，点对应卡片可以定位明细。预览和执行阶段分别加上本轮计划和复测数据。

```powershell
$scan = .\scripts\spacesweep.ps1 Scan -Arguments @('--discover') | ConvertFrom-Json
$analysis = .\scripts\spacesweep.ps1 Analyze -SessionDir $scan.session_dir `
    -Arguments @('--path','D:\','--discover','--max-seconds','20') | ConvertFrom-Json
$analysis.report_file
```

生成预览时用 `Plan -SessionDir $scan.session_dir -Arguments @('--scan',$scan.scan_id)`。确认后用同一会话的 `Apply` 传具体计划 ID。结果 JSON 会保留原生命令的字段、源数据文件和报告链接。

也可以用已有的原生命令 JSON 导出报告。多份目录分析通过 `-AnalysisJson` 数组传入。计划和执行输入分别使用 `plan` 和 `apply` 的标准输出 JSON。

```powershell
.\target\release\spacesweep.exe scan | Set-Content .spacesweep-state\scan.json -Encoding UTF8
.\target\release\spacesweep.exe scan --path 'D:\' --max-seconds 20 |
    Set-Content .spacesweep-state\disk-D.json -Encoding UTF8
.\scripts\report.ps1 -ScanJson .spacesweep-state\scan.json `
    -AnalysisJson .spacesweep-state\disk-D.json -OutputPath .spacesweep-state\report.html
```

可选的 `-PlanJson` 和 `-ApplyJson` 会显示清理估算、各盘实际变化和处理结果。HTML 可直接用浏览器打开，支持展开目录、最大文件和覆盖信息。输出使用新文件名。
