# 垃圾发现实机对照

2026-11-22 在同一台 Windows 11 电脑上，用标准用户权限对照 SpaceSweep `ad3f0f5` 和 BleachBit 6.0.6 便携版。两边均为只读扫描。SpaceSweep 默认检查 C、D、E 三盘。BleachBit 使用 `--preview` 检查指定清理项。 [BleachBit 版本](https://github.com/bleachbit/bleachbit/releases/tag/v6.0.6) · [预览命令](https://docs.bleachbit.org/doc/command-line-interface/#preview)

## 重合命中

提取 BleachBit 预览列出的文件路径，再读取普通文件长度。只在相同来源下比较数量和字节数。回收站按用量对照，SpaceSweep 的计数单位是可恢复条目，BleachBit 枚举其中的文件和目录。

| 来源 | SpaceSweep | BleachBit 同范围 |
| --- | ---: | ---: |
| Windows `thumbcache_*.db` | 15 个，1,215.55 MiB | 15 个，数量和字节数相同 |
| Chrome `Cache/Cache_Data` | 502 个，380.50 MiB | 502 个，数量和字节数相同 |
| Edge `Cache/Cache_Data` | 1,296 个，358.03 MiB | 1,296 个，数量和字节数相同 |
| C、D 盘回收站 | 1,122.86 MiB | 两盘合计字节数相同 |
| 红色旧临时文件 | 10 个，约 0.94 MiB | 10 个路径均列在临时文件预览中 |

## 实测差异

- BleachBit 额外列出了 Chrome 网页代码缓存约 321.03 MiB、Edge 网页代码缓存约 727.28 MiB，合计 1.02 GiB。两个浏览器的组件及扩展安装缓存合计约 348 MiB，也是本轮发现的覆盖缺口。
- SpaceSweep 找到用户 `CrashDumps` 的 10 个转储，约 359 MiB，以及 npm、pip 缓存约 1.09 GiB。本次所选的 BleachBit 预览里没有这些路径。
- BleachBit 的 `system.tmp` 还列入一处混有素材、备份和工具的临时工作目录，预览列出的普通文件约 3.54 GiB。其临时文件选项遇到一处 Windows 控制台编码错误，临时文件结果为部分覆盖。
- 本机相邻运行的 SpaceSweep 快查引擎用时 0.76 秒，BleachBit 的五个共同来源预览用时 3.25 秒。两次启动开销和实际探测范围不同。

## 复跑

从 SpaceSweep 项目目录运行快查。在 BleachBit 便携版目录先列出选项，再分别运行共同来源、临时文件和额外应用缓存的预览。浏览器在线时记录两边实际读取的大小。

```powershell
.\scripts\spacesweep.ps1 Scan -Arguments @('--discover')
.\bleachbit_console.exe --list-cleaners
.\bleachbit_console.exe --preview windows_explorer.thumbnails system.memory_dump system.recycle_bin google_chrome.cache microsoft_edge.cache
.\bleachbit_console.exe --preview system.tmp
.\bleachbit_console.exe --preview google_chrome.crash_reports microsoft_edge.crash_reports brave.cache firefox.cache discord.cache docker.cache winrar.temp vlc.memory_dump adobe_reader.cache
```

## 补齐网页代码缓存后复测

SpaceSweep 已将两款浏览器的 `Code Cache` 加到黄色审核项。复跑实机报告，Chrome 识别 3,957 个文件、336,624,184 字节，Edge 识别 3,707 个文件、762,605,049 字节，两边与对照记录的数量和字节数相同。快查阶段约 1.01 秒，三盘深查约 18.23 秒。

## 补齐 Edge 组件更新缓存后复测

在 Edge 156.0.4314.58 的默认用户数据根目录核对了 27 个组件更新包的 CRX 文件头与索引。改动前 C、D、E 三盘快查用时 599 毫秒，此目录未命中。同范围复跑用时 712 毫秒，新增一组黄色审核项，共 28 个文件、209,082,435 字节（199.4 MiB），与目录枚举和 BleachBit 对照记录一致。

## 通用应用缓存深查实测

C、D、E 三盘在同一条 `--discover --deep` 命令下复跑。去除项目隔离样本后，新增 248 组黄色缓存、45,595 个文件、7,698,518,894 字节（7.17 GiB）。识别结果包括 Steam、QQ、WeGame、Spotify、Obsidian、VS Code 和 D 盘 Steam 库中的应用网页缓存。D、E 合计约 351 MiB。既有快查来源与深查的同路径缓存归组没有重复。指定目录样本核对了 Firefox、迁移后的 npm 缓存以及 Chromium 桌面应用的布局。

改动前这一轮快查 0.70 秒、总计 81.51 秒。复跑快查 0.91 秒、总计 20.56 秒。上一轮记录的三盘深查为 18.23 秒，耗时以每轮观测值记录。

## 日常应用快查复测

2026-11-23 在同一台电脑对 C、D、E 执行只读 `Scan --discover`。原快查 0.517 秒、20 组。加入应用数据布局、安装位置和 Winapp2 路径后，本轮 4.080 秒、379 组。按类别和路径比对，新增 359 组、49,555 个文件、9,274.91 MiB。

| 新增来源 | 组数 | 逻辑占用 | 实机例子 |
| --- | ---: | ---: | --- |
| 按布局识别的应用缓存 | 332 | 8,518.66 MiB | QQ、微信、WeGame、Steam、Spotify、Obsidian 和 VS Code。D 盘 Wallpaper Engine 增加 17 组、339.33 MiB。 |
| Winapp2 路径线索 | 27 | 756.25 MiB | Adobe Camera Raw、Premiere Pro、Epic Games Launcher、Battle.net、EA App 和 OneDrive。 |

新增项目的用量来自实际文件大小。E 盘已检查登记位置，本轮无新增匹配。清理预览仍按已验证的 Rust 规则生成。

三盘深查复跑用时 42.372 秒，在 55 秒的本轮上限内结束。三盘均有跳过项。扣除项目样本后，深查报告中有 45 组、296.66 MiB 未出现在上述快查记录。核对类别、级别及路径，报告内无重复组。14 项 Rust 测试通过。
