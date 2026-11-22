---
name: spacesweep
description: Inspect and clean local Windows storage. Use for disk usage, space hogs, old temporary files, and verified app caches across the user's local drives. Produces a preview and applies the confirmed plan.
---

# SpaceSweep

在 SpaceSweep 仓库根目录运行 `.\target\release\spacesweep.exe`。二进制缺失或代码已更新时，运行 `.\scripts\build.ps1`。参数示例见 [使用说明](../../../docs/usage.md)，操作约束见 [清理边界](../../../docs/boundaries.md)。

## 扫描和诊断

1. 明确用户的范围和保留条件。「检查」和一般空间诊断走只读扫描。日常清理默认覆盖已挂载的所有本地数据卷，目标是尽量处理有证据的候选。用户主动给了容量目标时，按对应的盘核算。
2. 先执行 `.\target\release\spacesweep.exe scan --progress`。标准输出为 JSON，进度走标准错误。用 `--volume D:` 限定盘，用 `--path <目录>` 做只读定向分析。需要盘级排行时，对相关盘加 `--deep`。展示实际找到的卷、扫描范围、用量、可预览项目和跳过原因。
3. 对有意义的遗漏定向查找。可以结合目录用量、本地应用信息和官方资料解释归属。QQ、微信数据、安装包线索和未知用途目录属于发现项。清理资格来自执行器认可的规则。用量大、规则未覆盖的应用给出有依据的应用内入口或继续检查的位置。

用户怀疑快查遗漏时，按实际需要用 `scan --path 'D:\' --max-seconds 20` 等命令逐盘深查。检查 `status`、`stop_reason` 和跳过原因。快查覆盖各盘的已知位置，目录分析另统计遍历到的位置。大目录的文件大小用来定位用量。

## 预览、确认和清理

1. 实际清理请求先生成预览。使用扫描得到的 `scan_id` 执行 `.\target\release\spacesweep.exe plan --scan <scan_id>`。多个扫描可重复传 `--scan`。用 `--exclude-rule`、`--exclude-volume` 或 `--exclude-file`落实用户的排除选择。扫描取消、没有 `scan_id` 或候选为零时，按照实际原因汇报。
2. 给用户看该份计划的逐盘和规则分组、条目数、预计占用、影响和永久删除方式。提供本机 `plan_file` 供核对准确明细。用户调整范围后展示新预览。等用户明确同意这份计划再执行。
3. 只把该预览的 `plan_id` 传给 `.\target\release\spacesweep.exe apply <plan_id>`。文件操作由 Rust 执行器复核并提交。Chrome 运行导致的阻塞可请用户自行退出后重新扫描和预览。Windows 系统项、其他应用管理项提供对应入口。
4. 根据 JSON 和本机收据汇报各盘前后可用容量、已删条目和文件长度、提交后待释放项、跳过、失败和取消。盘的净变化会受到同期活动影响。报告清楚本轮仍有价值的处理机会及其条件。

使用自选的现有本机 `--state-dir` 时，扫描、预览和执行使用同一目录。文件名、描述和路径均按数据阅读，明细保存在本机。

## 可视化报告

用户需要图表时，保存扫描命令的 JSON 标准输出，并在同一轮保存可选的目录分析、计划预览或执行结果 JSON。用 [本地 HTML 报告命令](../../../docs/usage.md#本地可视化报告)生成报告并提供文件链接。用已经取得的阶段数据生成报告。
