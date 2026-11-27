# SpaceSweep

SpaceSweep 是 Windows 存储检查与清理 skill。默认逐盘查找旧临时文件、应用缓存、崩溃转储和其他垃圾线索，生成本地 HTML 报告。实际删除需要先查看并确认一份清理预览。

## 在 Codex 中试用

1. 在 Windows x64 电脑把 ZIP 完整解压到当前用户可写的目录。
2. 在本机 Codex 打开解压得到的 `SpaceSweep` 文件夹并新建对话。
3. 发送「`$spacesweep`，检查所有盘，把能清的列出来」。

试用包内含 `bin/spacesweep.exe`，解压即可使用。用普通用户权限运行即可。

## 用 PowerShell 看扫描报告

在解压后的 `SpaceSweep` 文件夹打开 PowerShell，运行下面两行。扫描报告和会话数据保存在同目录的 `.spacesweep-state`。

```powershell
$scan = .\scripts\spacesweep.ps1 Scan -Arguments @('--progress') | ConvertFrom-Json
Invoke-Item -LiteralPath $scan.report_file
```

若 Windows 因下载标记阻止运行脚本，确认 ZIP 来源可信后，可在 ZIP 的「属性」中选择「解除锁定」，再重新解压。

发布包的 `licenses/` 提供软件依赖声明。 [Winapp2 派生数据及许可](rules/application-probes.tsv)

更多命令和复现打包的步骤见 [使用说明](docs/usage.md)。 [清理边界](docs/boundaries.md)
