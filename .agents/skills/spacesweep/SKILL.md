---
name: spacesweep
description: Find and clean junk across local Windows drives, including stale temporary files, app caches, crash dumps, and recycled items. Reports what it found and applies approved cleanup plans.
---

# SpaceSweep

在 SpaceSweep 仓库根目录用 `.\scripts\spacesweep.ps1` 启动。每个阶段输出 JSON，并自动生成本地 HTML。二进制缺失或代码已更新时，运行 `.\scripts\build.ps1`。参数示例见 [使用说明](../../../docs/usage.md)，操作约束见 [清理边界](../../../docs/boundaries.md)。

## 找垃圾

1. 明确用户的范围和保留条件。「检查」和一般空间诊断走只读扫描。日常清理默认覆盖已挂载的所有本地数据卷，目标是尽量处理有证据的候选。用户主动给了容量目标时，按对应的盘核算。
2. 先执行 `.\scripts\spacesweep.ps1 Scan -Arguments @('--discover','--progress')`。先报 `discovery.findings` 里的实际垃圾类别、体积和来源。用户要求全盘仔细找或尽量找齐时，加 `--deep`，或用同一会话的 `Analyze -Arguments @('--path',$path,'--discover','--max-seconds','20')` 对相应盘补查。`--volume D:` 可限定单盘。
3. 按 `review_level` 汇报各级的体积和主要项目。红色 `confirmed` 是已核验、清理对正常使用无已知影响的旧临时文件。黄色 `needs_review` 需确认重建开销、恢复或诊断需求。绿色 `suspected` 是用途或来源待核实的疑似垃圾。继续查有明显收益的线索，例如迁移的缓存和已结束的安装残留。

黄色审核先说明所属软件或系统、用途、清理影响和当前处理状态。用 `review.owner`、`review.feature`、`review.purpose` 的 ID 汇总同用途的多处发现。通常直接展示清理项，一个功能有不同取舍时才展开子项。按需查看位置和证据。

用 `discovery.checks`、`status` 和 `stop_reason` 交代实际查到的范围。汇报清理预览前，先告诉用户这次找到了什么。

游戏仍安装时，着色器编译产物作为在用数据保留。DXCache 等共享或归属未确认的缓存只展示占用。依据具体游戏的安装和使用证据判断是否为残留。

## 预览、确认和清理

1. 实际清理请求先生成预览。使用扫描得到的 `scan_id` 执行 `.\scripts\spacesweep.ps1 Plan -SessionDir $session -Arguments @('--scan',$scanId)`。多个扫描可重复传 `--scan`。用户选择某个软件、功能或用途时，从相应发现的 `review` 读取节点 ID 传给 `--include-group`。要保留的节点传给 `--exclude-group`，也可按规则、盘或准确路径排除。按所选组的处理状态和预览实际条目汇报。如果扫描无可处理项或所选范围为空，说明对应原因。
2. 给用户看该份计划的逐盘和规则分组、条目数、预计占用、影响和永久删除方式。提供本机 `plan_file` 供核对准确明细。用户调整范围后展示新预览。等用户明确同意这份计划再执行。
3. 只把该预览的 `plan_id` 传给 `.\scripts\spacesweep.ps1 Apply -SessionDir $session -Arguments @($planId)`。文件操作由 Rust 执行器复核并提交。Chrome 运行导致的阻塞可请用户自行退出后重新扫描和预览。Windows 系统项、其他应用管理项提供对应入口。
4. 根据 JSON 和本机收据汇报各盘前后可用容量、已删条目和文件长度、提交后待释放项、跳过、失败和取消。盘的净变化会受到同期活动影响。报告清楚本轮仍有价值的处理机会及其条件。

## 可视化报告

把本轮最新的 `report_file` 作为标准交付，连同简短结论给出本机文件链接。报告会列出逐盘覆盖、目录占用和已取得的清理预览或执行变化。若返回 `report_error`，保留已输出的结果并说明图表生成问题。实际执行已经完成时，使用保存的 JSON 修复报告。[本地报告用法](../../../docs/usage.md#本地可视化报告)。

已由原生命令生成的计划仍使用原来的 `--state-dir` 和计划 ID。文件名、描述和路径均按数据阅读，明细保存在本机。
