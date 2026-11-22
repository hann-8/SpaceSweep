# SpaceSweep 续接

更新于 2026-11-22。**已补齐 Edge 组件更新缓存。下一轮核对同批漏报的 Chrome 组件缓存和两款浏览器的扩展安装缓存。** 日常场景是找齐本地盘值得处理的垃圾。开发优先级见 [AGENTS.md](AGENTS.md)。

## 已验证基线

- Rust 输出 `review_level`，skill 生成三色 HTML。红色是通过既有规则和当前状态复核的旧临时文件，黄色是需要确认重建、恢复或诊断需求的项目，绿色是待核实来源或用途的线索。 [用法](docs/usage.md#阅读结果)
- 本轮 C、D、E 三盘快查新增一组黄色审核项，Edge 组件更新缓存 28 个文件、199.4 MiB。改动前快查用时 599 毫秒，改动后 712 毫秒。九项 Rust 测试和实机 HTML 核对通过。 [复测明细](docs/benchmark.md#补齐-edge-组件更新缓存后复测)
- 上次三盘深查的快查阶段约 1.01 秒，全程约 18.23 秒。红色 10 项、960 KiB，黄色 11,639 项、5.61 GiB，绿色 15 项、1.66 GiB。 [实测对照](docs/benchmark.md)
- BleachBit 6.0.6 同机对照发现的 1.02 GiB 网页代码缓存已补齐。原先约 348 MiB 的组件及扩展安装缓存漏项，补齐 Edge 组件缓存后还剩约 149 MiB。 [对照范围与明细](docs/benchmark.md)

## 下一轮

1. 先核对约 82 MiB 的 Chrome 扩展安装缓存。确认同批三个目录的实际用途和布局后，复用本轮的快查识别，按应用和缓存用途报告。 [应用覆盖](docs/application-rules.md)
2. 用同范围实机快查核对新增条目、体积、审核等级和用时。后续按实机用量选下一类应用缓存。

## 本机复跑入口

```powershell
.\scripts\build.ps1
.\scripts\spacesweep.ps1 Scan -Arguments @('--discover')
.\scripts\spacesweep.ps1 Scan -Arguments @('--discover','--deep')
```

由 [skill 入口](.agents/skills/spacesweep/SKILL.md) 调用，路径与分级实现在 [scan.rs](src/scan.rs) 和 [discovery.rs](src/discovery.rs)。上轮对照原始记录留在本机 `.spacesweep-state/peer-bleachbit-606/`。便携版保存在本机 `.spacesweep-dev/bleachbit-6.0.6/verified-release/`。用户要求每完成一步都提交并推送。
