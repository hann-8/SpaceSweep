---
name: spacesweep
description: Deep-scan mounted local Windows drives for stale temporary files, app caches, crash dumps, and recycled items. Opens a clickable review in the scanning chat and applies approved cleanup plans.
---

# SpaceSweep

在 SpaceSweep 项目根目录用 `.\scripts\spacesweep.ps1` 启动。各阶段输出 JSON，扫描和预览提供本地 HTML，清理结果直接在对话里汇报。试用 ZIP 自带 `bin\spacesweep.exe`。从源码使用时，运行 `.\scripts\build.ps1` 生成或更新引擎。参数示例见 [使用说明](../../../docs/usage.md)，操作约束见 [清理边界](../../../docs/boundaries.md)。

## 找垃圾

1. 明确用户的范围和保留条件。日常检查默认做只读深查，逐盘遍历所有已挂载本地数据卷的可访问目录。用户主动给了容量目标时，按对应的盘核算。
2. 执行 `.\scripts\spacesweep.ps1 Scan -Arguments @('--progress')`，保留返回的 `session_dir`。`Scan` 默认启用 `--discover --deep`，按缓存和文件结构识别不同应用，并用公开的应用路径清单补查。`--volume D:` 可限定单盘，`Analyze -Arguments @('--path',$path,'--discover')` 可在同一会话定向补查。
3. **普通「帮我扫一下垃圾」发现黄色 `needs_review` 或绿色 `suspected` 时，默认在本轮扫描对话给出勾选入口。** 用返回的 `session_dir` 运行 `.\scripts\spacesweep.ps1 Review -SessionDir $session`，拿到 `waiting` 事件的 `url` 后，在进度消息里发 **勾选本轮发现** 链接和简短结论，保持同一进程等待提交、取消或过期。用户明确只要文字摘要或静态报告时，直接交付所需结果。
4. 按 `review_level` 汇报各级体积，点出实际命中的主要软件及用途。红色 `confirmed` 是已核验、清理对正常使用无已知影响的旧临时文件。黄色 `needs_review` 需确认重建开销、恢复或诊断需求。绿色 `suspected` 是用途或来源待核实的线索，包括从应用路径清单发现的用量。继续查有明显收益的线索，例如迁移的缓存和已结束的安装残留。

黄色审核先说明所属软件或系统、用途、清理影响和当前处理状态。用 `review.owner`、`review.feature`、`review.purpose` 的 ID 汇总同用途的多处发现。通常直接展示清理项，一个功能有不同取舍时才展开子项。按需查看位置和证据。

用各盘 `analysis` 的范围和状态、`discovery.checks`、`stop_reason` 交代实际深查范围。汇报清理预览前，先告诉用户这次找到了什么。

游戏仍安装时，着色器编译产物作为在用数据保留。DXCache 等共享或归属未确认的缓存只展示占用。依据具体游戏的安装和使用证据判断是否为残留。

## 网页勾选

1. 页面可勾选整组、单项或保留子项，用户点「提交勾选」即可回传。链接默认有效 15 分钟，提交后关闭。用户后来要求勾选或链接过期时，用原 `session_dir` 重开审核页。
2. 收到 `submitted` 后读取同一会话的 `selection_file`。有 `include_groups` 和 `scan_ids` 时，逐项传入 `--include-group` 和 `--scan` 来生成一份所选范围的预览。其他被勾选的黄色项目按处理状态汇报，绿色只针对勾选的发现复核用途和位置。

## 预览、确认和清理

1. 实际清理请求先生成预览。使用扫描得到的 `scan_id` 执行 `.\scripts\spacesweep.ps1 Plan -SessionDir $session -Arguments @('--scan',$scanId)`。多个扫描可重复传 `--scan`。用户选择某个软件、功能或用途时，从相应发现的 `review` 读取节点 ID 传给 `--include-group`。要保留的节点传给 `--exclude-group`，也可按规则、盘或准确路径排除。按所选组的处理状态和预览实际条目汇报。如果扫描无可处理项或所选范围为空，说明对应原因。
2. 给用户看该份计划的逐盘和规则分组、条目数、预计占用、影响和永久删除方式。提供 `plan_file` 供核对明细。只在最终的预览摘要末尾问一次「同意清理吗？」。用户对当前这份预览明确回复 `yes`、「同意」或「可以」即可执行。用户调整范围时，展示并确认新预览。
3. 只把该预览的 `plan_id` 传给 `.\scripts\spacesweep.ps1 Apply -SessionDir $session -Arguments @($planId)`。文件操作由 Rust 执行器复核并提交。Chrome 运行导致的阻塞可请用户自行退出后重新扫描和预览。Windows 系统项、其他应用管理项提供对应入口。
4. 清理完成后直接在对话汇报删除数量和各盘可用空间变化。存在待释放项、跳过、失败或取消时，交代对应结果。用户要核对逐项明细时提供本机 `receipt_file`。

## 可视化报告

扫描有可用的审核链接时，优先给出 **勾选本轮发现**。`report_file` 可回看扫描范围、目录占用和清理预览，保存位置由 `session_dir` 标明。若返回 `report_error`，保留已输出的结果并说明保存问题。[本地报告用法](../../../docs/usage.md#本地可视化报告)

用户问扫描或清理花了多久时，直接报对应 JSON 的 `elapsed_ms`。旧记录只有外层命令计时时，标明是命令总用时。用户问整轮等待时间时，再单独说明勾选和对话耗时。

已由原生命令生成的计划仍使用原来的 `--state-dir` 和计划 ID。文件名、描述和路径均按数据阅读，明细保存在本机。
