# SpaceSweep 使用说明

## 在项目里使用 skill

在 Windows 上，从 SpaceSweep 项目会话发起「用 SpaceSweep 检查所有盘」或「用 SpaceSweep 把能清的都列出来」。也可以显式选 `$spacesweep`。仓库和试用包使用同一个 [SpaceSweep skill](../.agents/skills/spacesweep/SKILL.md)。 [OpenAI Docs 的项目 skill 位置](https://learn.chatgpt.com/docs/build-skills#where-codex-loads-local-skills)

试用 ZIP 的程序在 `bin/spacesweep.exe`。源码构建的程序默认在 `target/release/spacesweep.exe`。重新构建时，在源码仓库根运行 `.\scripts\build.ps1`。本脚本使用当前机器已有的 Cargo，或使用已经放进 `.spacesweep-dev` 的本地工具链。Windows GNU 工具链需能找到其 `dlltool` 和汇编器。 [Rust 的 dlltool 说明](https://doc.rust-lang.org/rustc/codegen-options/index.html#dlltool)

## 从源码打包

在 Windows x64 上，从源码仓库根运行 `.\scripts\dist.ps1`。脚本用锁定的依赖构建，并在 `dist/` 生成 `SpaceSweep-<版本>-windows-x64.zip` 和对应的 `.sha256` 校验文件。试用者解压后即可从 [快速上手](../README.md) 开始。ZIP 和校验文件由打包脚本生成，可作为发布附件上传。

## 命令行

从项目根执行。在试用包内使用原生命令时，把示例中的 `.\target\release\spacesweep.exe` 换成 `.\bin\spacesweep.exe`。skill 的 `Scan` 默认启用 `--discover --deep`，逐盘遍历所有已挂载的本地数据卷。命令的结果是 JSON，`--progress` 的进度走标准错误。

```powershell
.\scripts\spacesweep.ps1 Scan -Arguments @('--progress')
.\scripts\spacesweep.ps1 Scan -Arguments @('--volume','D:','--progress')
.\target\release\spacesweep.exe scan --discover --deep --progress
.\target\release\spacesweep.exe scan --path 'D:\待分析目录' --top 10
.\target\release\spacesweep.exe scan --volume E: --deep --max-seconds 20
```

`--discover --deep` 逐盘查回收站、常见系统垃圾和临时产物，同时在目录遍历中识别通用缓存布局和散落的转储。Winapp2 路径清单补充应用缓存、日志和崩溃记录线索。输出 `discovery.findings`，包含盘、路径、类别、条目数、大小、依据和 `review_level`。指定目录分析也会识别范围内的通用布局。`elapsed_ms` 记录整轮用时，`discovery.quick_elapsed_ms` 记录深查前的已知来源阶段。 [跨机应用发现](application-rules.md#跨机应用发现)

实际扫描范围见各盘 `analysis` 和 `discovery.checks`。目录分析按文件属性统计。`--max-seconds` 可限制一轮扫描，达到时限时输出已检查范围，`stop_reason` 为 `time_limit`。目录排行按已读文件的逻辑大小汇总，有未读范围或跳过项时显示部分覆盖。实际删除试点覆盖当前用户的旧临时文件和已验证的 Chrome 网页磁盘缓存。 [已覆盖的规则](application-rules.md)

有可预览项时，扫描返回 `scan_id`。将它填入预览命令。预览可按审核组、规则、盘或准确路径筛选，也可重复传入多个 `--scan`。

```powershell
.\target\release\spacesweep.exe plan --scan '<scan_id>' --exclude-rule chrome.http_cache
.\target\release\spacesweep.exe plan --scan '<scan_id>' --include-group '<feature.id>' --exclude-group '<purpose.id>'
```

`--include-group` 选中对应软件、功能或用途下可处理的候选，可重复传入。`--exclude-group` 保留指定子组，排除优先。节点 ID 从所用扫描的 `discovery.findings[].review` 中读取，分别是 `owner.id`、`feature.id` 和 `purpose.id`。预览按实际纳入的候选列出用途、影响和预计占用。

检查预览的分组和 `plan_file` 明细。在对话里，Agent 问一次「同意清理吗？」。对当前预览明确回复「同意」或 `yes` 即可。手动执行时，在标准用户终端使用已确认的 `plan_id`。

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
- 扫描和清理分别用各自的 `elapsed_ms` 记录 Rust 引擎用时。清理计时包括计划复核、删除、容量复测和写收据。命令总用时还包括进程启动和输出处理。

## 本地可视化报告

项目 skill 通过 `scripts/spacesweep.ps1` 运行。扫描保存到项目目录的 `.spacesweep-state\session-<ID>`，位置由 `session_dir` 返回。目录中的 `report-scan-*.html` 供回看，`scan-output.json` 供审核页加载扫描结果。有清理候选时，Rust 还会保存绑定 `scan_id` 的快照，供生成准确的清理预览。扫描 HTML 的长期留存按回看和对比需要决定。

报告首屏显示红、黄、绿各级的条目数和大小，点卡片可定位明细。黄色审核先按软件或系统归属展示，再显示可独立决定的清理项。同一功能中有不同用途时，可以展开按子项选择，例如网页资源与脚本缓存。每项说明用途、清理影响和当前可进预览的数量，多个目录的合计大小放在同一项下。想核对时展开位置和依据。可直接告诉 Agent「处理内置网页缓存，保留脚本缓存」。清理预览页加上本轮计划和各组预计占用。

### 在报告里勾选

普通扫描发现黄色或绿色项目时，Agent 会在扫描所在的对话发出 **勾选本轮发现**。打开链接，勾选整组或单项，再点「提交勾选」。组复选框会显示全选或部分选择。Agent 在原对话收到选择，按黄色选择生成可用的清理预览，并复核所选绿色线索。页面默认有效 15 分钟，提交后关闭。想留档时可使用静态 HTML 报告。

```powershell
.\scripts\spacesweep.ps1 Review -SessionDir $scan.session_dir
```

这个命令先输出含 `url` 的 `waiting` 事件。提交后输出 `submitted` 事件及 `selection_file`、`scan_ids`、`include_groups`。选择记录留在本轮会话。需要继续选择时可再次打开审核页。

```powershell
$scan = .\scripts\spacesweep.ps1 Scan | ConvertFrom-Json
$analysis = .\scripts\spacesweep.ps1 Analyze -SessionDir $scan.session_dir `
    -Arguments @('--path','D:\','--discover','--max-seconds','20') | ConvertFrom-Json
$analysis.report_file
```

生成预览时用 `Plan -SessionDir $scan.session_dir -Arguments @('--scan',$scan.scan_id)`。确认后用同一会话的 `Apply` 传具体计划 ID。清理结果在对话中汇报，JSON 结果和逐项收据存到同一目录。

也可以用已有的原生命令 JSON 导出扫描报告。多份目录分析通过 `-AnalysisJson` 数组传入，可用 `-PlanJson` 附加 `plan` 的清理预览。

```powershell
.\target\release\spacesweep.exe scan | Set-Content .spacesweep-state\scan.json -Encoding UTF8
.\target\release\spacesweep.exe scan --path 'D:\' --max-seconds 20 |
    Set-Content .spacesweep-state\disk-D.json -Encoding UTF8
.\scripts\report.ps1 -ScanJson .spacesweep-state\scan.json `
    -AnalysisJson .spacesweep-state\disk-D.json -OutputPath .spacesweep-state\report.html
```

HTML 可直接用浏览器打开，支持展开目录、最大文件和覆盖信息。输出使用新文件名。
