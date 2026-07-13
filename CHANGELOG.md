# 更新日志

本项目从 `0.2.0` 开始维护面向开发者和发布使用者的变更记录。格式参考 Keep a Changelog；预览版可能继续调整内部协议，稳定版发布前必须明确迁移策略。

## [未发布]

### 计划

- Windows 自动化任务调度、重试和并发策略。
- 更新包签名、更新通道和离线补丁目录。
- RPAZ 能力权限、依赖扩展包和包来源治理。
- 受控的 AI Agent 辅助开发与 RPAZ 工具调用。

## [0.2.0-preview.10] - 2026-07-14

### 新增

- Windows x64 无注册表 NSIS 引导安装器，支持选择非系统盘目录。
- `.drpa-update` 文件级更新器：清单与 SHA-256 校验、暂存、原子替换、备份、失败回滚和重启。
- 任意页面拖拽一个或多个 `.rpaz` 安装。
- 脚本包右键菜单：运行、复制到工作室、卸载。
- 运行环境页面：查看状态和路径、初始化、验证、修复。
- 工作室直接运行源码、打开已安装包副本和基于名称自动生成项目/包 ID。
- 真实 Jupyter/IPython Kernel：标准输出、执行结果、图片、HTML、错误、变量浏览和 notebook 输出回写。
- Bing 每日一图 `0.2.0` 示例包与独立 SHA-256 文件。

### 修复

- 修复 Windows 离线 Python 误选 `Lib\\venv\\scripts\\nt\\python.exe` 后报 `No pyvenv.cfg file` 的严重问题。运行时现在从受校验清单读取精确解释器路径。
- Windows GUI 和 Jupyter/worker 子进程使用无控制台窗口启动参数，不再先弹出 CMD。
- 修复自定义窗口拖动、缩放、最大化、最小化和关闭交互。
- 修复设置、工作台参数和多个仅展示但没有 Host 行为的入口。

### 交付变化

- 离线运行时和桌面 Release 收敛为 Windows x64；暂不发布 Linux/macOS 安装包。
- GitHub Actions 在最终便携/安装目录结构中初始化 Python 环境并导入运行时、浏览器和 Jupyter 依赖。
- GitHub Actions 启动真实 Jupyter/ZMQ Kernel，连续执行两个单元并校验状态保持。
- 安装脚本 CI 禁止 `WriteReg`、`DeleteReg`、`ReadReg` 和 `InstallDirRegKey`。
- 工作数据固定保存在安装目录的 `data/`，更新器禁止覆盖该目录。

### 已知限制

- preview-10 是热更新基线；更旧版本仍需重新安装一次。
- 当前 `.drpa-update` 由用户在设置中本地选择，尚未自动发现或下载更新。
- 预览包未签名；转移到离线机器前后必须验证配套 SHA-256。
- Jupyter 尚不覆盖远程 Kernel、ipywidgets、VS Code 调试器和完整扩展生态。

[0.2.0-preview.10]: https://github.com/EthanBird/drpa-client/releases/tag/desktop-v0.2.0-preview-10
