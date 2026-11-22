# SpaceSweep 续接

更新于 2026-11-22。**下一步从实机发现的浏览器缓存漏项里挑一项，核实后补上识别，并用同范围扫描复跑。** 日常场景仍是找齐所有本地盘值得处理的垃圾。开发优先级见 [AGENTS.md](AGENTS.md)。

## 已验证基线

- 当前功能基线是 `master` 的 `db86c6f`。Rust 输出 `review_level`，skill 生成三色 HTML。红色是通过既有规则和当前状态复核的旧临时文件，黄色是需要确认重建、恢复或诊断需求的项目，绿色是待核实来源或用途的线索。九项 Rust 测试、Clippy、skill 校验和实机 HTML 渲染通过。 [用法](docs/usage.md#阅读结果)
- 本机 Windows 11 的 C、D、E 完成一次只读深查，快查阶段约 1.01 秒，全程约 18.23 秒。红色 10 项、960 KiB，黄色 11,639 项、5.61 GiB，绿色 15 项、1.66 GiB。实际清理仍走获批的具体预览。 [执行边界](docs/boundaries.md#授权与执行)
- BleachBit 6.0.6 的同机预览验证了缩略图、Chrome 和 Edge 网页缓存、回收站的同范围用量，以及 10 个红色临时文件路径。发现的 1.02 GiB 网页代码缓存已补齐并复测对上。组件及扩展安装缓存仍有约 348 MiB 待核实。BleachBit 的临时目录选项部分覆盖，双方速度数据的范围也不同。 [选项、耗时与明细](docs/benchmark.md)

## 下一轮

1. 从已测得的约 348 MiB 组件及扩展安装缓存中，选一处能确认用途的目录。复现当前漏项，补上只读识别和审核等级。 [实测对照](docs/benchmark.md)
2. 用同范围扫描实机复跑，记录新增命中、大小、等级和用时。实机难以判断的误报或误标红，用少量有已知答案的样本验证，复用 [现有垃圾识别测试](src/scan.rs) 即可。

## 本机复跑入口

```powershell
.\scripts\build.ps1
.\scripts\spacesweep.ps1 Scan -Arguments @('--discover')
.\scripts\spacesweep.ps1 Scan -Arguments @('--discover','--deep')
```

由 [skill 入口](.agents/skills/spacesweep/SKILL.md) 调用，路径与分级实现在 [scan.rs](src/scan.rs) 和 [discovery.rs](src/discovery.rs)。上轮对照原始记录留在本机 `.spacesweep-state/peer-bleachbit-606/`。便携版保存在本机 `.spacesweep-dev/bleachbit-6.0.6/verified-release/`。用户要求每完成一步都提交并推送。
