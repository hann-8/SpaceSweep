# SpaceSweep 续接

更新于 2026-11-23。**日常快查已覆盖应用数据、各盘已登记的安装位置和 912 条社区路径。** 已识别的缓存和路径线索可在扫描所在对话勾选。预览后只问一次「同意清理吗？」。清理结果直接回到对话，逐项收据留在本机。开发优先级见 [AGENTS.md](AGENTS.md)。

## 已验证基线

- C、D、E 日常快查从 20 组增至 379 组，本轮用时 4.08 秒。新增约 9.06 GiB，含 D 盘 Wallpaper Engine 约 339 MiB。深查复跑 42.37 秒，继续发现 45 组。本轮报告无重复组，14 项 Rust 测试通过。 [实测](docs/benchmark.md#日常应用快查复测)
- 空计划执行验证了独立 `elapsed_ms` 和本机收据。扫描和预览提供 HTML，执行在对话汇报。 [使用说明](docs/usage.md#阅读结果)
- Rust 输出 `review_level`，skill 生成三色 HTML。红色是通过既有规则和当前状态复核的旧临时文件，黄色是需要确认重建、恢复或诊断需求的项目，绿色是待核实来源或用途的线索。 [用法](docs/usage.md#阅读结果)
- 实测快查的静态报告曾漏掉审核入口，现将启动审核放进 skill 的日常扫描步骤。用同一份快查数据验证审核页有 12 个黄色、6 个绿色复选框和提交按钮。此前已在 Chrome 验证整组勾选、取消子项、黄绿混选及提交回传，Rust 样本验证预览范围取自所选黄色项目。 [网页勾选](docs/usage.md#在报告里勾选)
- 上轮实机报告将 QQ 内置网页与客户端网页分开，分别汇总资源和脚本。Edge 的组件更新包单列。隔离样本验证批量选择、保留子项与准确的计划明细。 [分组与预览](docs/usage.md#本地可视化报告)
- 本轮识别 Chromium 桌面应用、Firefox 和 npm 通用布局。扣除项目测试目录，C、D、E 深查新增 248 组缓存，共 45,595 个文件、7.17 GiB，含 D、E 上约 351 MiB。快查阶段 0.91 秒，整轮 20.56 秒。10 项 Rust 测试通过。 [实测明细](docs/benchmark.md#通用应用缓存深查实测)
- BleachBit 6.0.6 同机对照发现的 1.02 GiB 网页代码缓存已纳入快查。Edge 组件更新缓存的单项识别作为浏览器对照样本，实机覆盖 28 个文件、199.4 MiB。 [对照范围与明细](docs/benchmark.md)

## 下一轮

1. 推进整类应用缓存从发现到可处理。用本轮常见桌面应用的缓存样本，核实共同布局、应用内清理入口与使用影响。验收看用户确认后能处理的范围、体积和整轮用时。
2. 后续按系统盘空间告急、剪辑和游戏等高占用应用的场景交付。使用真实应用布局和对应处理入口，一轮复跑验收一类能力。 [使用场景](docs/design.md#按真实场景交付)

## 本机复跑入口

```powershell
.\scripts\build.ps1
.\scripts\spacesweep.ps1 Scan -Arguments @('--discover')
.\scripts\spacesweep.ps1 Scan -Arguments @('--discover','--deep')
```

由 [skill 入口](.agents/skills/spacesweep/SKILL.md) 调用，路径与分级实现在 [scan.rs](src/scan.rs) 和 [discovery.rs](src/discovery.rs)。上轮对照原始记录留在本机 `.spacesweep-state/peer-bleachbit-606/`。便携版保存在本机 `.spacesweep-dev/bleachbit-6.0.6/verified-release/`。用户要求每完成一步都提交并推送。
