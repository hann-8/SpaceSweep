# SpaceSweep 规则与应用覆盖

## 规则怎样描述

首版用本地声明式规则描述应用差异。识别和执行共用 Rust 的目录定位、规则校验、大小统计、计划和文件复核。确认删除资格需要具体版本、用途和范围的依据。

| 规则信息 | 内容 |
| --- | --- |
| 身份与版本 | 独立规则编号、修订号、应用标识及适用的 Windows 软件版本或安装类型。 |
| 来源与证据 | 可核对的厂商文档、应用设置或在该版本上复现的验证。安装记录只辅助定位。 |
| 根目录与匹配 | Rust 支持的可信定位方式，明确的相对路径和匹配范围。支持 Windows 已知目录、经验证的应用目录和应用实际配置位置。 |
| 条件与影响 | 保留设置、应用是否需退出、受保护的相邻数据、重建耗时或可能的重新下载和登录影响。 |
| 动作 | Rust 定义的固定选项，例如统计用量、列入文件清理预览、提供应用内入口。规则的字段值按数据解析。 |

调研从实际占用和可验证性出发。先核实软件版本与当前用户配置，再复现候选与应保留资料位于同级时的行为。验证后的项目规则随版本发布。用途不确定或版本对不上时，可报告用量及待验证事项。新增规则同时核对实际数据盘与应用自带的清理方式。复用上游代码、规则或资源前核对许可证。

## 通用项目

| 项目 | 识别与处理方式 |
| --- | --- |
| 旧的用户临时文件 | 首先验证当前用户的 LocalAppData 下的 `Temp`。创建与最后写入时间均早于保留期的普通文件可进入预览，先以 7 天试验。应用在用、安装或恢复资料跳过。 |
| 下载的独立安装包 | 展示位置和大小，用户确定已用完后可以逐项选择。系统的安装缓存依[清理边界](boundaries.md)处理。 |
| 游戏与驱动的着色器编译产物 | 游戏仍安装时保留。DXCache 等共享或归属未确认的目录只统计占用。仅在证实具体文件属于已卸载游戏、且其他应用未使用时，才作为待审查残留。 |
| 临时空目录 | 用完整枚举确认当前为空，在明确的生成目录和移除条件下才进入预览。其他空目录可作为发现项。 |

## 已实现的垃圾发现

`scan --discover` 在清理预览前就能识别下列对象。候选用量按实际读到的文件大小或 Windows 查询值汇总。

| 来源 | 发现范围 |
| --- | --- |
| 回收站 | 通过 Windows 接口逐盘查询条目数与大小。 [SHQueryRecycleBinW](https://learn.microsoft.com/en-us/windows/win32/api/shellapi/nf-shellapi-shqueryrecyclebinw) |
| 崩溃转储 | 当前用户默认 `CrashDumps` 里的 `.dmp`。深查也识别散落的 `程序.exe.进程号.dmp`，用开头四字节确认 Windows 进程转储格式。 [Windows Error Reporting](https://learn.microsoft.com/en-us/windows/win32/wer/collecting-user-mode-dumps) · [转储文件头](https://learn.microsoft.com/en-us/windows/win32/api/minidumpapiset/ns-minidumpapiset-minidump_header) |
| 缩略图缓存 | 资源管理器目录里的 `thumbcache_` 数据库。 [Microsoft 缩略图机制](https://learn.microsoft.com/en-us/windows/win32/shell/thumbnail-providers) · [BleachBit 缩略图路径](https://github.com/bleachbit/bleachbit/blob/master/cleaners/windows_explorer.xml) |
| 网页缓存 | Chrome、Edge 默认用户配置中的 `Cache/Cache_Data` 和 `Code Cache`。浏览器运行时也可统计。代码缓存列为黄色，清理后访问网页可能重新编译脚本。 [Chromium 生成代码缓存](https://chromium.googlesource.com/chromium/src/+/main/content/browser/code_cache/generated_code_cache_context.h) |
| 通用应用缓存布局 | 深查和指定目录分析识别 Chromium 浏览器与桌面应用的磁盘缓存和脚本缓存、Firefox 的 `cache2` 和 npm 的 `_cacache`。按目录结构与索引或内容目录识别，可覆盖迁移到其他盘的配置。结果列为黄色。 [Electron 会话数据](https://www.electronjs.org/docs/latest/api/app#appgetpathname) · [Firefox HTTP 缓存](https://firefox-source-docs.mozilla.org/networking/cache2/doc.html) · [npm 缓存](https://docs.npmjs.com/cli/cache/) |
| Edge 组件更新缓存 | 当前用户默认 Edge 用户数据根目录下的 `component_crx_cache`。统计顶层文件名由 64 个十六进制字符组成的更新包和 `metadata.json`，列为黄色。清理后组件更新时可能需要重新下载。 [Chromium CRX 缓存格式](https://chromium.googlesource.com/chromium/src/+/refs/heads/main/components/update_client/crx_cache.cc) |
| 软件包缓存 | npm 的 `_cacache`，pip 的 HTTP 和 wheel 缓存。在当前用户默认缓存位置查找。 [npm](https://docs.npmjs.com/cli/cache/) · [pip](https://pip.pypa.io/en/stable/topics/caching/) |
| 临时产物 | 递归找超过七天的 `.tmp`。记录安装格式或程序名含 Setup、Installer 的线索，以及旁边存在同名目录的压缩包。 |

## 应用覆盖路线

先支持浏览器磁盘缓存的规则验证，并优先识别 QQ、微信实际占用。以下是逐步覆盖的对象和处理方向。报告以本机版本和路径的实际验证结果为准。

当前可执行规则随二进制嵌入 `rules/windows.toml`。旧临时文件首版限于当前用户 LocalAppData 的 `Temp` 顶层普通 `.tmp` 文件。Chrome 规则限于 157 主版本的默认配置位置，要求检测到已验证的块文件布局且 Chrome 已退出。只匹配 `Cache/Cache_Data` 里的 `index`、`data_数字` 和 `f_六位十六进制` 普通文件。用实际 Chrome 的隔离配置验证了布局、清除后重建和样本 Cookie 保留。Edge、聊天数据和安装包按发现项展示。 [Chromium 块文件实现](https://chromium.googlesource.com/chromium/chromium/+/HEAD/net/disk_cache/blockfile/backend_worker_v3.cc)

| 软件举例 | 优先检查与处理方向 |
| --- | --- |
| QQ、微信、企业微信、钉钉、飞书、Teams | 用已知位置、应用配置或用户指定目录做用量检查，定位实际数据盘。聊天记录、收发文件、账号和备份按用户资料处理。独立可重建的缓存需要按版本核实。Teams 的缓存清理适合特定问题，优先展示官方入口。 [微信隐私保护指引](https://oper.my-qcloud.com/cgi-bin/readtemplate?lang=zh_CN&s=privacy&t=weixin_agreement) · [Teams 缓存说明](https://learn.microsoft.com/en-us/troubleshoot/microsoftteams/teams-administration/clear-teams-cache) |
| Chrome、Edge、Firefox、Brave | 核实各版本和用户配置下的网页磁盘缓存。Cookie、密码、历史、书签和离线网站数据属于用户资料。 [Chrome](https://support.google.com/chrome/answer/2392709/clear-browsing-data-computer) · [Edge](https://support.microsoft.com/en-us/edge/view-and-delete-browser-history-in-microsoft-edge) · [Firefox](https://support.mozilla.org/en-US/kb/how-clear-firefox-cache) |
| Steam、Epic Games Launcher、WeGame | 检查启动器或下载缓存，以及它们和游戏、更新任务、存档的边界。Steam 清除下载缓存后需要重新登录，适合在应用内确认。 [Steam 支持](https://help.steampowered.com/en/faqs/view/6AD7-820D-8BE5-E51F) · [Epic 支持](https://www.epicgames.com/help/c-32735058/a20673770?lang=en-US) |
| Spotify、网易云音乐、QQ 音乐 | 识别在线播放缓存和离线下载的占用，提示是否需要重新下载。Spotify 提供缓存管理入口。 [Spotify 存储说明](https://support.spotify.com/us/article/storage-information/) |
| Premiere、After Effects、剪映、达芬奇 | 核实用户设置的媒体缓存、波形、预处理音频或预览渲染位置。工程、自动保存、源素材和成品作为项目资料。依赖项目引用关系时使用应用内入口。 [Premiere](https://helpx.adobe.com/premiere/desktop/troubleshooting/media-issues/clear-media-cache-using-preferences.html) · [After Effects](https://helpx.adobe.com/after-effects/desktop/get-started/preferences-and-settings/preferences.html) |
| Word、WPS、Outlook | 查可归属的占用，按用途保护文档、模板、自动恢复和邮件数据。可重建的子项按版本验证。 [Word 自动恢复](https://support.microsoft.com/en-us/word/change-save-frequency-and-where-word-autorecovery-files-are-stored) |
| OneDrive、百度网盘、Dropbox、迅雷 | 查本地占用。同步、离线下载和未完成任务由应用处理。OneDrive 提供「释放空间」，将适用的文件转为仅联机。 [OneDrive 按需文件](https://support.microsoft.com/en-gb/onedrive/save-disk-space-with-onedrive-files-on-demand-for-windows) |
| VS Code、JetBrains、pip、pnpm、Docker | 核实下载、构建缓存与扩展、项目、数据卷的区别。优先验证 pip 下载缓存。未引用的 pnpm 包和未使用的 Docker 构建缓存交给工具按自身规则判断。 [pip](https://pip.pypa.io/en/stable/topics/caching/) · [pnpm](https://pnpm.io/cli/store#prune) · [Docker](https://docs.docker.com/engine/manage-resources/pruning/) |

Epic 可验证同一应用的两种处理方式。旧版用规则记录厂商明确的网页缓存和退出条件，Rust 检查版本、路径、状态和清理对象。Preview 版由应用内执行「清理并重启」，规则提供入口和影响。 [Epic 官方说明](https://www.epicgames.com/help/c-32735058/a20673770?lang=en-US)

## 上游参考

BleachBit 的 CleanerML 可参考按应用和用途写规则、共享深度扫描以及独立预览。Winapp2.ini 可作为发现软件与候选路径的调查资料。候选清理范围仍需独立验证。 [CleanerML](https://docs.bleachbit.org/cml/cleanerml/) · [BleachBit 预览](https://docs.bleachbit.org/doc/command-line-interface/#preview) · [Winapp2.ini](https://github.com/MoscaDotTo/Winapp2)
