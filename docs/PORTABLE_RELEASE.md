# DRPA Next 离线版

本发布包已经包含 DRPA 桌面端、平台专用 Python 3.11 运行时、锁定依赖、Chrome for Testing 和 Bing 每日一图示例包，不需要联网安装 Python 或 pip 依赖。

## 启动

- Windows：解压完整 ZIP 后运行 `DRPA Next.exe`。这是便携版，不使用 NSIS/MSI，不写入注册表；内置的 Fixed Version WebView2 也通过进程环境变量加载。首次启动会为 WebView2 文件夹设置只读/执行 ACL，并在用户数据目录离线创建 Python 环境。
- Linux：解压完整归档，保持 `DRPA Next.AppImage` 与 `runtime` 同级，然后运行 `./DRPA Next.AppImage`。
- macOS：打开 DMG，将 `DRPA Next.app` 拖到任意可写目录后启动。当前预览版未使用 Apple Developer ID 公证，首次启动可能需要在 Finder 中右键选择“打开”。

不要只复制可执行文件或移动 `runtime` 子目录。首次初始化耗时取决于磁盘速度，过程完全离线；后续启动会复用校验通过的环境。

每个下载文件都带有独立的 `.sha256`。在离线机器转移前后应核对散列值。
