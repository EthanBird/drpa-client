# DRPA Next Windows 离线版

当前只提供 Windows x64，包含 DRPA 桌面端、CPython 3.11、完整离线依赖、真实 Jupyter Kernel、Chrome for Testing、Fixed Version WebView2 和 Bing 每日一图示例包。目标机器无需预装 Python、Node.js、浏览器或联网下载 pip 依赖。

## 首次安装

1. 下载 `drpa-next-<version>-windows-x86_64-setup.exe` 和对应 `.sha256`。
2. 在联网区与离线机器分别核对 SHA-256。
3. 运行图形安装向导，选择非系统盘目录。
4. 启动后打开“运行环境”并执行初始化/验证。
5. 安装 Bing 示例 RPAZ，运行一次并检查日志和图片产物。

安装器不申请管理员权限、不注册卸载项、不读写应用注册表，只释放文件并创建 `.lnk` 快捷方式。CI 会扫描并拒绝 NSIS 中的注册表指令。内置 WebView2 通过进程级配置加载。

应用、运行时和用户数据均在安装目录：

```text
<install>/
├── DRPA Next.exe
├── runtime/
├── webview2/
└── data/                 包、项目、环境、历史、产物和更新状态
```

不要只复制 `DRPA Next.exe`、单独移动 `runtime/` 或把已经初始化的 `data/runtime-environment` 复制到另一条路径。首次初始化会在最终安装位置创建可用环境；后续启动复用校验通过的环境。

## 文件级更新

preview-10 是当前热更新基线。更旧版本没有 `drpa-updater.exe`，必须先重新安装 preview-10 或更新版本一次。

后续更新步骤：

1. 下载 `.drpa-update` 与对应 `.sha256`。
2. 核对散列。
3. 打开“设置 → Windows 文件级热更新”，选择更新包。
4. 应用完成后程序自动重启；失败则恢复备份并保留诊断状态。

Host 会检查平台、schema、版本、安全路径、文件大小和 SHA-256。更新器只替换清单列出的应用文件，明确拒绝覆盖 `data/` 和更新器自身；因此包、项目、运行历史和环境不会因应用更新被清空。

当前更新包由用户本地选择，尚未实现自动联网检查、数字签名和块级差分。预览版也未进行代码签名；只使用可信 GitHub Release，并在转移前后验证散列。

## 卸载与备份

由于不写注册表，系统“应用和功能”中不会出现注册卸载项。退出 DRPA 后，备份需要保留的 `data/`，再删除整个安装目录和快捷方式即可。

升级或迁移前建议备份整个 `data/`。恢复到不同目录后先运行环境验证；如果 Python 环境损坏，使用“修复运行环境”重建 `data/runtime-environment/`，不要删除 `packages/`、`projects/` 或 `runs/`。

## 发布资产

Windows Release 应严格包含六个文件：

- Setup EXE 与 SHA-256。
- `.drpa-update` 与 SHA-256。
- Bing 每日一图示例 `.rpaz` 与 SHA-256。

开发和发布流程见 [`DEVELOPMENT.md`](DEVELOPMENT.md)，完整依赖处理见 [`../offline/README.md`](../offline/README.md)。
