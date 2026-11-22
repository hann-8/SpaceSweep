# SpaceSweep 续接

更新于 2026-11-23。**已交付黄、绿审核项的本机网页勾选。** 用链接打开本轮报告，勾选整组或单项后回传同一会话。黄色按既有资格生成预览，绿色交给 Agent 按选择复核。下一阶段推进日常整轮清理的覆盖与预览。开发优先级见 [AGENTS.md](AGENTS.md)。

## 已验证基线

- Rust 输出 `review_level`，skill 生成三色 HTML。红色是通过既有规则和当前状态复核的旧临时文件，黄色是需要确认重建、恢复或诊断需求的项目，绿色是待核实来源或用途的线索。 [用法](docs/usage.md#阅读结果)
- 用已有 C、D、E 实机报告在 Chrome 验证了整组、取消子项、黄绿混选和提交回传。收到的选择保留了 QQ 网页资源缓存以及一条绿色线索。Rust 样本验证可预览的范围来自所选黄色项目。 [网页勾选](docs/usage.md#在报告里勾选)
- 上轮实机报告将 QQ 内置网页与客户端网页分开，分别汇总资源和脚本。Edge 的组件更新包单列。隔离样本验证批量选择、保留子项与准确的计划明细。 [分组与预览](docs/usage.md#本地可视化报告)
- 本轮识别 Chromium 桌面应用、Firefox 和 npm 通用布局。扣除项目测试目录，C、D、E 深查新增 248 组缓存，共 45,595 个文件、7.17 GiB，含 D、E 上约 351 MiB。快查阶段 0.91 秒，整轮 20.56 秒。10 项 Rust 测试通过。 [实测明细](docs/benchmark.md#通用应用缓存深查实测)
- BleachBit 6.0.6 同机对照发现的 1.02 GiB 网页代码缓存已纳入快查。Edge 组件更新缓存的单项识别作为浏览器对照样本，实机覆盖 28 个文件、199.4 MiB。 [对照范围与明细](docs/benchmark.md)

## 下一轮

1. 做好日常整轮清理。对照常见清理软件的日常入口，在一次使用里覆盖系统及用户临时文件、回收站和常见应用缓存。把主要发现、能进预览的用量和下一步动作放到首屏。用首批结果时间、整轮用时和实际发现体积验收。
2. 后续按系统盘空间告急、剪辑和游戏等高占用应用的场景交付。使用真实应用布局和对应处理入口，一轮复跑验收一类能力。 [使用场景](docs/design.md#按真实场景交付)

## 本机复跑入口

```powershell
.\scripts\build.ps1
.\scripts\spacesweep.ps1 Scan -Arguments @('--discover')
.\scripts\spacesweep.ps1 Scan -Arguments @('--discover','--deep')
```

由 [skill 入口](.agents/skills/spacesweep/SKILL.md) 调用，路径与分级实现在 [scan.rs](src/scan.rs) 和 [discovery.rs](src/discovery.rs)。上轮对照原始记录留在本机 `.spacesweep-state/peer-bleachbit-606/`。便携版保存在本机 `.spacesweep-dev/bleachbit-6.0.6/verified-release/`。用户要求每完成一步都提交并推送。
