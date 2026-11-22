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
