# DRPA Next

DRPA Next 是一个 Windows 优先、本地优先的可扩展代码包运行管理器。用户只需安装一次桌面应用，即可在不配置系统 Python 的情况下安装、开发、运行、观察和更新 `.rpaz` 自动化脚本包。

当前版本为 `0.2.0` Windows x64 离线预览版。原有 PySide6 客户端已冻结，只作为 `.rpaz` v1 行为和迁移参考；新功能只进入 Tauri + React + Rust 架构。

## 当前能力

- 中文默认界面，支持正常移动、缩放、最大化、最小化和关闭窗口。
- 引导式、无注册表写入的 Windows NSIS 安装器；应用和用户数据均可放在非系统盘安装目录。
- 内置 CPython 3.11、完整离线 wheels、Chrome for Testing、Fixed Version WebView2 和真实 Jupyter Kernel 依赖。
- 安装、拖拽导入、运行、取消和卸载 `.rpaz`；包操作集中在右键菜单。
- 工作室可新建项目、编辑源码和 Notebook、直接运行工作副本、导出 `.rpaz`，也可将已安装包复制为可编辑项目。
- 基于 `ipykernel`、`jupyter_client`、`pyzmq`、`nbformat` 的真实 Jupyter 执行链路。
- 清单差量 `.drpa-update` 更新，包含可视化进度、散列校验、直接文件替换、失败回滚和末段自动重启；日常更新不重复携带 WebView2/Chrome，并始终保护安装目录下的 `data/`。

Windows 预览版下载：<https://github.com/EthanBird/drpa-client/releases/tag/desktop-v0.2.0-preview-10>

> 旧安装首次升级仍由旧更新器执行；新轻量包不再重复携带 runtime/浏览器文件。首次过渡成功后，后续更新使用包内独立 Worker 和可视化进度；若旧版过渡失败，运行最新 Setup 建立新基线。

## 架构边界

```text
React / TypeScript UI
        │ typed Tauri invoke
        ▼
Rust Host ── package / runtime / run / update policy
        │ versioned JSONL + Jupyter wire protocol
        ▼
Sealed Python 3.11 ── RPAZ worker / IPython kernel / Chrome
```

- `apps/desktop/`：Tauri 2 桌面壳、React UI、窗口和更新器。
- `crates/drpa-package/`：manifest、归档和路径安全规则。
- `crates/drpa-host/`：包、运行记录和 Host 领域服务。
- `crates/drpa-protocol/`：前后端与运行时共享 DTO/事件协议。
- `runtime/python/`：RPAZ Python adapter、Runtime Context 和 Jupyter bridge。
- `offline/`：离线运行时规范、精确依赖锁和引导脚本。
- `installer/windows/`：无注册表 NSIS 安装器。
- `tools/windows/`：Windows 文件级更新包生成器。

UI 不是安全边界。所有文件路径、包清单、更新清单和运行请求都必须由 Rust Host 再次验证。

## 本地开发

要求 Node.js 24+、Rust stable 和 Tauri 2 的 Windows 构建依赖。前端浏览器预览使用确定性的 mock gateway，不需要启动 Rust Host：

```bash
npm ci
npm run dev
npm run typecheck
npm run test
npm run build
```

Windows 桌面联调：

```bash
npm run tauri:dev
```

Rust 与 Python 验证：

```bash
cargo fmt --all --check
cargo clippy -p drpa-protocol -p drpa-package -p drpa-host --all-targets -- -D warnings
cargo test -p drpa-protocol -p drpa-package -p drpa-host
python -m pip install -e "./runtime/python[test]"
python -m pytest -q runtime/python/tests
python tools/offline/validate_requirements.py offline/requirements/runtime.txt
```

正式 Windows 安装包必须由 GitHub Actions 的原生 Windows runner 构建和进行最终安装布局验证，不要将本地前端 build 当成离线发布验收。

## 文档导航

- [开发与交接手册](docs/DEVELOPMENT.md)：当前实现、目录、数据、测试、发布和接手清单。
- [功能扩展路线](docs/ROADMAP.md)：离线基础环境、自动化任务和 AI Agent 辅助开发。
- [RPAZ 开发](docs/RPAZ_DEVELOPMENT.md)：schema v2、Runtime Context、直接运行和示例包。
- [Jupyter 集成](docs/JUPYTER_INTEGRATION.md)：真实能力、VS Code Jupyter 对照和明确边界。
- [离线运行时](offline/README.md)：依赖策略、构建证明和缺包处理流程。
- [Windows 发布说明](docs/PORTABLE_RELEASE.md)：安装、数据目录和热更新。
- [架构设计](docs/architecture/DRPA_NEXT.md)：长期模块边界和安全原则。
- [更新日志](CHANGELOG.md)：面向发布和接手者的变更记录。

## 当前限制

- 只发布 Windows x64；Linux/macOS 仍是未来适配目标，不属于当前交付承诺。
- 当前更新入口使用本地 `.drpa-update`，已支持逐文件差量；尚未实现在线更新源、签名信任链和大文件块级差分。
- Jupyter 使用真实协议，但不是完整 VS Code Extension Host；远程 Kernel、ipywidgets、VS Code 调试器和所有第三方 MIME renderer 尚未实现。
- 自动调度、浏览器录制器、包签名/私有仓库和 AI Agent 均处于设计阶段，详见路线文档。
