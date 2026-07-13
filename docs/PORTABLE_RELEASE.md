# DRPA Next 离线版

本发布包已经包含 DRPA 桌面端、平台专用 Python 3.11 运行时、锁定依赖、Chrome for Testing 和 Bing 每日一图示例包，不需要联网安装 Python 或 pip 依赖。

## 启动

- Windows：运行 Setup EXE，通过图形向导选择非系统盘目录。安装器不申请管理员权限、不注册卸载项、不读写应用注册表，只释放文件并创建 `.lnk` 快捷方式；内置 Fixed Version WebView2 通过进程环境变量加载。项目、脚本包、运行历史和 Python 环境均保存在安装目录下的 `data`。
- Linux：解压完整归档，保持 `DRPA Next.AppImage` 与 `runtime` 同级，然后运行 `./DRPA Next.AppImage`。
- macOS：打开 DMG，将 `DRPA Next.app` 拖到任意可写目录后启动。当前预览版未使用 Apple Developer ID 公证，首次启动可能需要在 Finder 中右键选择“打开”。

不要只复制可执行文件或移动 `runtime` 子目录。首次初始化耗时取决于磁盘速度，过程完全离线；后续启动会复用校验通过的环境。

每个下载文件都带有独立的 `.sha256`。在离线机器转移前后应核对散列值。
