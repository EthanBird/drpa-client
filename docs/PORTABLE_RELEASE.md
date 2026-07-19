# DRPA Next Windows 离线版

当前只提供 Windows x64，包含 DRPA 桌面端、CPython 3.11、完整离线依赖、真实 Jupyter Kernel、Chrome for Testing、Fixed Version WebView2 和 Bing 每日一图示例包。目标机器无需预装 Python、Node.js、浏览器或联网下载 pip 依赖。

## 首次安装

1. 下载 `drpa-next-<version>-windows-x86_64-setup.exe`。
2. 运行图形安装向导，选择非系统盘目录。
3. 启动后打开“运行环境”并执行初始化/验证。
4. 安装 Bing 示例 RPAZ，运行一次并检查日志和图片产物。

安装器不申请管理员权限、不注册卸载项、不读写应用注册表，只释放文件并创建 `.lnk` 快捷方式。CI 会扫描并拒绝 NSIS 中的注册表指令。内置 WebView2 通过进程级配置加载。

应用、运行时和用户数据均在安装目录：

```text
<install>/
├── DRPA Next.exe
├── drpa-updater.exe
├── install-manifest.json  protocol-2 完整受管文件库存
├── runtime/
├── webview2/
└── data/                 包、项目、环境、历史、产物和更新状态
```

不要只复制 `DRPA Next.exe`、单独移动 `runtime/` 或把已经初始化的 `data/runtime-environment` 复制到另一条路径。首次初始化会在最终安装位置创建可用环境；后续启动复用校验通过的环境。

## 文件级更新

`0.3.0` 全量安装包是新的热更新基线。`0.2.x` 及更早安装没有 schema-2 库存和 protocol-2 启动确认，直接运行最新 Setup 完成迁移；不要使用轻量包跨越该边界。完成全量安装后，后续版本均使用包内 Worker。

后续更新步骤：

1. 下载 `.drpa-update`。
2. 打开“设置 → Windows 文件级热更新”，选择更新包。
3. 更新窗口会显示包结构读取、Worker 就绪、当前文件与完成比例；Host 只有确认 Worker 独立运行后才短暂关闭。
4. Worker 等待文件锁释放后替换清单文件，从安装目录启动新版本，并保留备份直到新 Host 确认主窗口已创建；早退、确认超时或替换失败时恢复备份、重新打开旧版本并保留错误记录。

Host 会检查平台、schema、Host/Worker protocol、最低 Host 版本、精确基线版本、安全路径、文件数量与写入大小，不执行逐文件哈希验证。更新器只替换清单列出的应用文件，并明确保护 `data/` 和 WebView2；因此包、项目、运行历史和环境不会被日常应用更新波及。

完整安装包必须携带 CPython、Chrome 和 Fixed Version WebView2，保证目标离线机器首次安装即可运行；这些大组件不会在每个热更新包中重复。安装根目录的 `install-manifest.json` 是完整文件库存。普通 push 默认只发布轻量 update：主程序、更新器、Python adapter wheel、运行时 bootstrap 和合并后的完整库存。adapter wheel 变化时客户端只原位刷新该小包，不重建 pandas、Jupyter、DrissionPage 等完整依赖。手工选择 `full`，或使用 `release(windows):` 提交前缀准备稳定版本时，才重新生成 Setup、完整 runtime、WebView2 和示例资产。

发布 stage 内的文件必须是实体文件。库存生成器拒绝 symlink、Junction 和 reparse point，防止安装包在构建机器上通过、复制到离线机器后才暴露缺失依赖。

当前更新包由用户本地选择，尚未实现自动联网检查、数字签名和块级差分。预览版也未进行代码签名；更新文件从项目 GitHub Release 获取。

## 卸载与备份

由于不写注册表，系统“应用和功能”中不会出现注册卸载项。退出 DRPA 后，备份需要保留的 `data/`，再删除整个安装目录和快捷方式即可。

升级或迁移前建议备份整个 `data/`。恢复到不同目录后先运行环境验证；如果 Python 环境损坏，使用“强制重建生成环境”，在确认窗口核对影响后重建 `data/runtime-environment/`，不要删除 `packages/`、`projects/` 或 `runs/`。

## 发布资产

日常 update Release 应严格包含两个文件：

- `.drpa-update`。
- `install-manifest.json`。

full 基线 Release 包含 Setup EXE、`install-manifest.json` 与 Bing 每日一图示例 `.rpaz`，共三个文件。全量基线不附带 `.drpa-update`；下一次日常发布才以该库存为基线生成轻量包。

开发和发布流程见 [`DEVELOPMENT.md`](DEVELOPMENT.md)，完整依赖处理见 [`../offline/README.md`](../offline/README.md)。
