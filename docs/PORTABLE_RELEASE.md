# DRPA Next 离线版

本次发布只提供 Windows x64，已经包含 DRPA 桌面端、Python 3.11 运行时、真实 Jupyter Kernel 依赖、Chrome for Testing 和 Bing 每日一图示例包，不需要联网安装 Python 或 pip 依赖。

## 启动

- 运行 Setup EXE，通过图形向导选择非系统盘目录。安装器不申请管理员权限、不注册卸载项、不读写应用注册表，只释放文件并创建 `.lnk` 快捷方式；内置 Fixed Version WebView2 通过进程环境变量加载。项目、脚本包、运行历史和 Python/Jupyter 环境均保存在安装目录下的 `data`。
- 后续升级可以在“设置 → Windows 文件级热更新”选择 `.drpa-update`。更新器只替换清单列出的应用文件，校验失败会停止，替换失败会回滚；不会覆盖 `data` 或运行安装器。

不要只复制可执行文件或移动 `runtime` 子目录。首次初始化耗时取决于磁盘速度，过程完全离线；后续启动会复用校验通过的环境。

每个下载文件都带有独立的 `.sha256`。在离线机器转移前后应核对散列值。
