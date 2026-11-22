# SpaceSweep 的清理边界

## 默认范围

日常请求的目标是把电脑里能处理的项目尽量清一遍。默认检查 Windows 当前挂载的所有本地数据卷，包括系统盘、其他内置盘和已连接的外置数据盘。网络映射盘按网络资源识别。可以限定盘或目录，也可以在同一份跨盘计划中排除盘、应用和项目。指定盘腾出或达到某个容量是用户主动提出时使用的场景。 [Windows 驱动器类型](https://learn.microsoft.com/en-us/windows/win32/api/fileapi/nf-fileapi-getdrivetypew) · [卷与挂载](https://learn.microsoft.com/en-us/windows/win32/fileio/naming-a-volume)

扫描和清理以当前 Windows 用户的标准权限运行。卷按身份去重，通过已核实的入口各自扫描。递归遍历时跳过重解析点，包括目录联接、符号链接和子目录中的卷挂载点。离线占位文件、权限不足、不可用卷的状态和跳过原因进入报告。读取和执行能力分别按实测声明。 [枚举 Windows 卷](https://learn.microsoft.com/en-us/windows/win32/fileio/enumerating-volumes) · [重解析点](https://learn.microsoft.com/en-us/windows/win32/fileio/reparse-point-operations)

## 哪些内容可以处理

| 内容 | 处理边界 |
| --- | --- |
| 旧临时文件、可重建缓存 | 由项目验证过的规则匹配具体产物、版本、可信目录和保留条件。已知用于安装、恢复或仍在使用的项目跳过。重建、重新登录或重新下载代价较大的项目说明代价后由用户选择。 |
| 游戏着色器编译产物 | 游戏仍安装时按在用产物保留。共享缓存或归属无法确定的着色器只统计占用。只有核实具体归属、关联游戏已卸载且无其他使用方时，才作为待审查残留。 |
| 空目录 | 读取时检查所有条目，包括隐藏和系统条目。无法枚举时记为未知。只对归属明确、允许移除的临时子目录生成清理计划，执行时复核为空并使用非递归删除。 |
| 个人文件、收发附件、聊天记录、备份、工程、邮件 | 按资料保护，发现占用时提供位置和用途供用户决定。有明确来源的独立下载安装包可报告给用户逐项选择。独立安装副本与系统用于更新、修复、卸载的安装缓存用途不同。 |
| 系统组件、Windows 更新、回收站、注册表、应用卸载和同步 | 交给系统或应用自身的管理入口。报告说明适用入口及影响。 |
| 用途未知的大文件、疑似重复文件 | 可只读检查并定位。文件名、扩展名、年龄或大小提供调查线索。清理资格来自已验证的规则和用户选择。 |

微软将安装缓存用于后续维护，微信记录存于用户终端。有关操作使用对应的系统和应用功能。 [Windows Installer 缓存](https://learn.microsoft.com/en-us/troubleshoot/windows-client/application-management/missing-windows-installer-cache) · [Windows 存储清理](https://support.microsoft.com/zh-cn/windows/experience/storage-filemanagement/free-up-drive-space-in-windows) · [微信隐私保护指引](https://oper.my-qcloud.com/cgi-bin/readtemplate?lang=zh_CN&s=privacy&t=weixin_agreement)

## 授权与执行

- 实际删除以用户确认的预览为准。计划绑定范围、选中的准确条目、规则版本、目标身份和处理方式。预览展示大小、原因、重建代价和是否永久删除。后续新增的候选需要进入新的预览。
- Rust 在提交前重新核对规则、应用状态、可信根、卷、文件身份、类型、元数据、最终路径和路径中的重解析点，并通过已核验的同一个句柄提交删除。遇到更改、占用、多个硬链接或无法验证的对象，留下具体原因并跳过。
- 取消后停止提交新操作，保留已执行结果。重复提交同一计划时，以当前文件身份和计划记录为准。已处理、缺失或有变动的条目记录为相应状态。
- 计划和明细留在本机，Agent 按需读取摘要或少量选定条目。路径、文件名、应用配置和上游规则内容均按数据处理。查外部资料时使用公开的软件名称和版本。

执行接口和适用文件系统需通过实测核验。 [文件身份](https://learn.microsoft.com/en-us/windows/win32/api/winbase/ns-winbase-file_id_info) · [最终路径](https://learn.microsoft.com/en-us/windows/win32/api/fileapi/nf-fileapi-getfinalpathnamebyhandlew) · [按句柄提交文件状态](https://learn.microsoft.com/en-us/windows/win32/api/fileapi/nf-fileapi-setfileinformationbyhandle) · [删除状态与句柄关闭](https://learn.microsoft.com/en-us/windows/win32/api/winbase/ns-winbase-file_disposition_info)
