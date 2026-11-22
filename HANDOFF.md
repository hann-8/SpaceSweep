# SpaceSweep 续接

更新于 2026-11-22。**下一步先做有已知答案的垃圾识别样本，跑出当前版本的漏报、误报和用时基线。** 日常场景仍是找齐所有本地盘值得处理的垃圾。开发优先级见 [AGENTS.md](AGENTS.md)。

## 已验证基线

- 当前功能基线是 `master` 的 `db86c6f`。Rust 输出 `review_level`，skill 生成三色 HTML。红色是通过既有规则和当前状态复核的旧临时文件，黄色是需要确认重建、恢复或诊断需求的项目，绿色是待核实来源或用途的线索。九项 Rust 测试、Clippy、skill 校验和实机 HTML 渲染通过。 [用法](docs/usage.md#阅读结果)
- 本机 Windows 11 的 C、D、E 完成一次只读深查，快查阶段约 1.01 秒，全程约 18.23 秒。红色 10 项、960 KiB，黄色 11,639 项、5.61 GiB，绿色 15 项、1.66 GiB。实际清理仍走获批的具体预览。 [执行边界](docs/boundaries.md#授权与执行)
- BleachBit 6.0.6 的同机预览验证了缩略图、Chrome 和 Edge 网页缓存、回收站的同范围用量，以及 10 个红色临时文件路径。发现的 1.02 GiB 网页代码缓存已补齐并复测对上。组件及扩展安装缓存仍有约 348 MiB 待核实。BleachBit 的临时目录选项部分覆盖，双方速度数据的范围也不同。 [选项、耗时与明细](docs/benchmark.md)

## 下一轮怎么验收

1. 从 [现有垃圾识别测试](src/scan.rs) 的 `discovery_finds_temporary_artifacts_without_counting_other_files` 起步。先给一套小型隔离样本写期望清单，记录路径、所属盘、文件数、逻辑大小和预期等级或保留结果。期望来自样本的已知用途，BleachBit 结果用于提供补查线索。
2. 样本混放通过现有规则的旧 `.tmp`、网页与代码缓存、散落转储、用途待定的安装包和同名压缩包，以及近期临时文件、聊天附件、备份和仍安装游戏的着色器。实机样本放到系统盘和可用数据盘的隔离目录，覆盖迁移后的应用位置。用测试目录核验固定应用路径，定向 `--path` 用于复测散落线索。
3. 先跑当前版本，逐组核对 `discovery.findings` 的路径、条目数、文件大小和等级。记录各类命中、漏报、误报、误标红和跳过项，同时记录快查与深查耗时。后续横比速度时统一扫描范围、计时起点和缓存条件。交付可重跑的样本及结果表。 [验收设计](docs/design.md#实现顺序与验收)
4. 根据结果优先补收益明显的漏项。先查清组件及扩展安装缓存的用途，再用 Windows 自带预览和常用应用的存储管理确认各自的发现范围。 [已覆盖的来源](docs/application-rules.md)

## 本机复跑入口

```powershell
.\scripts\build.ps1
.\scripts\spacesweep.ps1 Scan -Arguments @('--discover')
.\scripts\spacesweep.ps1 Scan -Arguments @('--discover','--deep')
```

由 [skill 入口](.agents/skills/spacesweep/SKILL.md) 调用，路径与分级实现在 [scan.rs](src/scan.rs) 和 [discovery.rs](src/discovery.rs)。上轮对照原始记录留在本机 `.spacesweep-state/peer-bleachbit-606/`。便携版保存在本机 `.spacesweep-dev/bleachbit-6.0.6/verified-release/`。用户要求每完成一步都提交并推送。
