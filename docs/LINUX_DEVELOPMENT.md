# DRPA Next Linux 开发与移植交接

本文面向接手 Linux 桌面端的开发者，记录当前代码已经具备的跨平台基础、仍属于 Windows 的实现、可直接复现的 Linux 开发流程，以及把 Linux 从“可编译”推进到“可离线发布”所需的验收清单。

## 1. 当前结论

DRPA Next `1.0.0` 的正式发行物仍是 Windows x64 全量离线安装包。Linux 端不是从零开始，但现阶段也不是可对终端用户发布的产品。

| 能力 | Linux 当前状态 | 证据或入口 |
| --- | --- | --- |
| React/Vite 浏览器开发 | 可用 | `npm run dev` 使用 `mockGateway` |
| Rust core | CI 覆盖 | `drpa-protocol`、`drpa-package`、`drpa-host` 在 Ubuntu 测试 |
| Tauri Host 编译 | CI 覆盖 | Ubuntu 安装 WebKitGTK 后执行 `cargo check -p drpa-desktop` |
| Tauri 桌面联调 | 可在本机进行 | `npm run tauri:dev`，需要图形会话和 WebKitGTK |
| Linux 数据目录与 `xdg-open` | 已有实现 | `app_local_data_dir()/workspace`、`open_directory_in_file_explorer` |
| Linux x86_64 runtime 规格 | 已声明 | `offline/runtime-spec.json` 的 `linux-x86_64` |
| Linux sealed runtime 构建器 | 已有通路 | `tools/offline/build_runtime_bundle.py` 支持 `linux-x86_64` |
| Linux sealed runtime CI/Release | 待接入 | workflow 目前只包含 `windows-x86_64` |
| AppImage / deb 最终布局 | 待实现和验收 | Tauri 能生成包，但 runtime 尚未进入平台安装布局 |
| Linux 文件级热更新 | 未实现 | 当前命令、协议与独立 Worker 只接受 `windows-x86_64` |
| Linux GUI、浏览器、Jupyter 端到端 | 待加入 CI | 目前没有 Linux 最终包和离线机器验收 |

因此，Linux 开发应分成两个阶段：先用源码模式打通真实 Tauri Host，再完成 sealed runtime、最终包布局和原生发行验收。不要把一次 `cargo check` 或单独生成的 AppImage 当作交付完成。

## 2. 目标基线

第一阶段只处理 **Linux x86_64 + glibc**：

- 构建基线：Ubuntu 22.04；它能提供 WebKitGTK 4.1，也能降低 AppImage 的 glibc 最低版本。
- 验证系统：至少 Ubuntu 22.04 和 Ubuntu 24.04 的干净虚拟机。
- 首选发行格式：AppImage；deb 在 runtime 资源路径确定后再加入。
- Python：CPython `3.11.9`。
- Node.js：`24`，以 root `package.json` 和 CI 为准。
- Rust：stable，最低 Rust 版本以 workspace `Cargo.toml` 为准。
- uv：`0.11.28`，以 `offline/runtime-spec.json` 为准。
- UI WebView：Linux 使用系统 WebKitGTK，不携带 Windows WebView2。
- 自动化浏览器：平台 sealed runtime 中的 Chrome for Testing，不依赖用户系统 Chrome。

Tauri 官方 Linux 前置要求与 AppImage 兼容性说明：

- <https://v2.tauri.app/start/prerequisites/>
- <https://v2.tauri.app/distribute/appimage/>

## 3. Ubuntu / Debian 开发环境

### 3.1 系统依赖

Ubuntu 22.04 或 Debian 12 可使用：

```bash
sudo apt update
sudo apt install -y \
  libwebkit2gtk-4.1-dev \
  build-essential \
  curl \
  wget \
  file \
  libxdo-dev \
  libssl-dev \
  libayatana-appindicator3-dev \
  librsvg2-dev \
  patchelf \
  xdg-utils
```

CI 当前使用的最小编译依赖见 `.github/workflows/ci.yml`。上面的列表按 Tauri 2 官方开发前置要求补齐了本地运行和打包常用组件。

随后安装 Node.js 24、Rust stable 与 Python 3.11.9。建议用版本管理器安装，不要修改仓库中的版本约束来迁就本机旧工具。

```bash
node --version
npm --version
rustc --version
cargo --version
python3.11 --version
```

### 3.2 拉取与安装前端依赖

```bash
git clone https://github.com/EthanBird/drpa-client.git
cd drpa-client
git switch codex/drpa-next-platform
npm ci
```

旧 `src/drpa_client/`、root `pyproject.toml` 和 `scripts/run-drpa.sh` 属于 PySide6 兼容参考。Linux 新功能进入 `apps/desktop/`、`crates/` 和 `runtime/python/`，不要从旧启动脚本开始移植。

## 4. 三种开发模式

### 4.1 只开发 React UI

```bash
npm run dev
```

访问 Vite 输出的本地地址。此模式使用 `apps/desktop/src/infra/mockGateway.ts`，适合页面、布局和组件测试，不覆盖文件系统、进程、sealed runtime 或 Tauri command。

### 4.2 运行真实 Linux Tauri Host

先准备仓库根目录的开发 Python 环境。为了与发行运行时一致，建议安装精确依赖集合：

```bash
python3.11 -m venv .venv
./.venv/bin/python -m pip install --upgrade pip
./.venv/bin/python -m pip install -r offline/requirements/runtime.txt
./.venv/bin/python -m pip install -e './runtime/python[test]'
```

设置显式开发路径，避免把测试数据写入真实用户目录：

```bash
export DRPA_DATA_DIR="$PWD/.drpa-data"
export DRPA_RUNTIME_PYTHON="$PWD/.venv/bin/python"
export DRPA_RUNTIME_PYTHONPATH="$PWD/runtime/python/src"

# 运行浏览器 RPA 时设置；仅打开桌面 UI 或编辑文档时可以省略。
export DRPA_BROWSER_PATH="$(command -v google-chrome || command -v chromium || command -v chromium-browser)"

npm run tauri:dev
```

`DRPA_RUNTIME_PYTHON` 与 `DRPA_RUNTIME_PYTHONPATH` 必须成对使用。Debug Host 原本也会尝试仓库根目录 `.venv/bin/python`，显式设置更容易诊断路径问题。

### 4.3 使用 Linux sealed runtime 联调

已有 runtime 目录时，不再设置开发 Python，而是指定 runtime 根目录：

```bash
unset DRPA_RUNTIME_PYTHON DRPA_RUNTIME_PYTHONPATH DRPA_BROWSER_PATH
export DRPA_DATA_DIR="$PWD/.drpa-data-sealed"
export DRPA_RUNTIME_ROOT="$PWD/offline-build/runtime"
npm run tauri:dev
```

`DRPA_RUNTIME_ROOT` 必须直接指向包含 `manifest.json`、`bootstrap_runtime.py`、`python/`、`wheelhouse/` 和 `browser/` 的目录。Host 会验证 manifest 的平台值必须为 `linux-x86_64`，并在数据目录中创建可写的 `runtime-environment/environment`。

## 5. Linux 平台合同

### 5.1 数据目录

Linux 未设置 `DRPA_DATA_DIR` 时，Host 使用：

```text
app.path().app_local_data_dir()/workspace
```

实际根目录由 Tauri 按 XDG 规则解析。开发和自动化测试应总是设置 `DRPA_DATA_DIR`，这样清理测试数据不会触碰真实用户资料。

数据子目录与 Windows 保持同一领域结构：

```text
workspace/
├── packages/
├── projects/
├── build/
├── runs/
├── runtime-environment/
├── knowledge/
├── agent/
└── updates/
```

Linux 发布包必须保持用户数据与只读应用文件分离。卸载或替换应用时不得删除 XDG 数据目录。

### 5.2 WebView 与自动化浏览器

这两个浏览器角色不能混淆：

- Tauri UI 由系统 WebKitGTK 渲染。
- RPA 脚本通过 DrissionPage 使用 `DRPA_BROWSER_PATH` 指向的 Chrome for Testing。

Linux 包不应携带 WebView2；Chrome for Testing 仍属于 sealed runtime。开发机只有在运行浏览器脚本时才需要配置 Chrome/Chromium。

### 5.3 打开本地目录

Linux Host 使用 `xdg-open` 打开工作区、构建输出和任务产物。发行包应依赖或检测 `xdg-utils`。在无桌面会话、容器或纯 SSH 环境中，目录打开动作应返回清晰错误，不能影响任务本身的成功状态。

### 5.4 进程与取消

Python worker、Jupyter kernel 和浏览器可能创建子进程。Linux 发布前必须把“取消任务”验证到完整进程树，而不只是杀死直接 child。建议把平台差异收敛到进程 adapter：Linux 使用独立 process group，并对 group 发送终止信号；业务层保持同一取消语义。

### 5.5 更新能力

当前 `apply_windows_update`、`drpa-updater`、更新清单 target 和重启确认协议均为 Windows 实现。Linux 第一版可只提供完整 AppImage 替换，但 UI 必须根据平台能力隐藏 Windows 更新入口。不要让 Linux 用户选择 `.drpa-update` 后才收到 Windows-only 错误。

## 6. 构建 Linux sealed runtime

构建器已经支持 `linux-x86_64`，但必须在原生 Linux x86_64、Python 3.11.9 上执行：

```bash
./.venv/bin/python -m pip install 'uv==0.11.28'
./.venv/bin/python tools/offline/validate_requirements.py offline/requirements/runtime.txt
./.venv/bin/python tools/offline/build_runtime_bundle.py \
  --platform linux-x86_64 \
  --work-dir "$PWD/offline-build/linux-x86_64"
```

构建阶段需要网络，用于下载受控 Python、精确 wheels 和固定 Chrome for Testing。构建器随后会在空 uv cache、禁止联网的条件下：

1. 创建全新环境；
2. 验证 adapter 与依赖；
3. 用 DrissionPage 启动内置 Chrome 并访问本地 HTML；
4. 启动真实 Jupyter Kernel 连续执行两个单元；
5. 生成 `manifest.json`、文件库存、归档与归档校验文件。

输出目录位于：

```text
offline-build/linux-x86_64/out/
```

把归档解压后，将内部 `drpa-runtime-...-linux-x86_64/` 目录作为 `DRPA_RUNTIME_ROOT` 即可进行 Host 联调。正式 workflow 接入前，先确保该命令在 Ubuntu 22.04 的全新 runner 上可重复成功。

## 7. Tauri Linux 包

探索性构建命令：

```bash
npm run tauri:build --workspace @drpa/desktop -- --bundles appimage,deb
```

这条命令成功只证明 Tauri 应用可以打包。当前 `tauri.conf.json` 没有把 Linux sealed runtime 放入 AppImage/deb 的最终资源布局，单独生成的包仍不满足离线交付要求。

在确定正式布局前，可以这样联调 AppImage：

```bash
chmod +x path/to/DRPA-Next_1.0.0_amd64.AppImage
DRPA_RUNTIME_ROOT="$PWD/runtime" \
DRPA_DATA_DIR="$PWD/.drpa-appimage-data" \
  path/to/DRPA-Next_1.0.0_amd64.AppImage
```

正式布局需要一次明确设计决策：

- AppImage 是把 runtime 作为外部同目录资产，还是作为可定位的只读资源携带；
- deb 把 runtime 安装到哪个只读目录；
- `runtime_roots()` 如何通过 Tauri resource API 定位，而不是假设 `/usr/bin/runtime`；
- 应用升级时如何保留 XDG 数据并重建或复用生成环境；
- Chrome sandbox、文件权限和可执行位如何在归档与安装后保持正确。

在这些问题完成前，不要上传 Linux Release。

## 8. 本地验证命令

### 8.1 前端

```bash
npm ci
npm run typecheck
npm run test
npm run build
```

### 8.2 Rust

```bash
cargo fmt --all --check
cargo clippy -p drpa-protocol -p drpa-package -p drpa-host --all-targets -- -D warnings
cargo test -p drpa-protocol -p drpa-package -p drpa-host
cargo check -p drpa-desktop
cargo test -p drpa-desktop
```

### 8.3 Python 与离线政策

```bash
./.venv/bin/python -m pytest -q runtime/python/tests tools/windows/tests
./.venv/bin/python -m unittest discover -s tools/offline/tests -v
./.venv/bin/python -m compileall -q tools/offline offline/bootstrap runtime/python/src
./.venv/bin/python tools/offline/validate_requirements.py offline/requirements/runtime.txt
./.venv/bin/python tools/release/check_version_consistency.py --expected 1.0.0
```

`tools/windows/tests` 在 Linux CI 运行的是更新包的纯 Python 格式与策略测试，不代表 Windows Worker 已在 Linux 上变成可用能力。

## 9. 推荐实施顺序

### 里程碑 A：源码模式可用

- 在 Ubuntu 22.04 图形会话启动 `npm run tauri:dev`。
- 工作区、知识文档、Studio 文件操作和 `xdg-open` 正常。
- 使用开发 Python 运行普通 RPAZ、Bing 示例和两个 Notebook 单元。
- 设置页隐藏或禁用 Windows-only 能力，并显示准确平台状态。

### 里程碑 B：sealed runtime 可复现

- 把 `linux-x86_64` 加入独立 runtime workflow matrix。
- 在原生 Ubuntu runner 完成 air-gap bootstrap、DrissionPage 和 Jupyter smoke。
- 发布 runtime artifact，但暂不发布桌面端。
- 修正所有写死“Windows 运行时”的诊断文案。

### 里程碑 C：最终包布局

- 先完成 AppImage，随后再完成 deb。
- 用 Tauri resource path 或明确安装前缀定位 runtime。
- 最终包不依赖系统 Python、系统 Chrome、npm 或网络。
- 数据目录遵守 XDG，应用移动或升级不损坏用户数据。

### 里程碑 D：原生验收与发布

- 在 Ubuntu 22.04 构建，在 Ubuntu 22.04/24.04 干净虚拟机测试。
- 覆盖 X11 与 Wayland 至少各一次人工 GUI 验收。
- 验证中文、空格和非 ASCII 路径。
- 验证任务取消后 Python/Jupyter/Chrome 进程树全部退出。
- 验证断网首次初始化、Bing 本地流程、Notebook、知识库导入导出和 Agent 本地工具。
- Linux Release 使用独立平台资产名和平台清单，不复用 Windows `.drpa-update`。

## 10. Linux 发布验收标准

Linux 端只有同时满足以下条件才进入 Release：

1. 干净系统无需安装 Python、Node、Rust 或 Chrome 即可启动并运行 RPAZ。
2. 首次运行断网可完成 runtime 初始化，后续运行也不触发 pip/uv 网络请求。
3. AppImage 或 deb 能稳定定位与自身匹配的 `linux-x86_64` runtime manifest。
4. UI 使用 WebKitGTK，自动化使用内置 Chrome，两者升级边界清晰。
5. 数据写入 XDG 目录；应用目录保持只读仍可正常运行。
6. Studio 源码、RPAZ 导出、Bing 示例、Jupyter 两单元和产物目录打开均通过。
7. 长任务日志实时输出，取消后完整进程树退出。
8. 最终包在 Ubuntu 22.04 与 24.04 通过安装、启动、移动、升级和卸载回归。
9. CI 保存 runtime 构建证明、最终包和机器可读安装库存。
10. README、Release notes 和应用设置页不再把未实现的 Windows 更新能力显示为 Linux 可用。

## 11. 常见问题

### `javascriptcoregtk` / `webkit2gtk` 找不到

确认安装的是 WebKitGTK **4.1** 开发包，并执行：

```bash
pkg-config --modversion webkit2gtk-4.1
cargo check -p drpa-desktop
```

### 运行环境页面提示找不到封装运行时

源码联调使用第 4.2 节的 `DRPA_RUNTIME_PYTHON`；sealed runtime 联调使用 `DRPA_RUNTIME_ROOT`。不要把 Windows runtime 复制到 Linux。

### DrissionPage 找不到浏览器

开发模式设置 `DRPA_BROWSER_PATH`。sealed 模式只信任 runtime manifest 的 `browserExecutable`，不要在 Host 中递归搜索浏览器。

### AppImage 能打开，但运行脚本失败

先检查是否同时提供了匹配的 runtime，并查看：

```bash
echo "$DRPA_RUNTIME_ROOT"
cat "$DRPA_RUNTIME_ROOT/manifest.json"
```

单独的 AppImage 当前不是完整离线产品包。

### GUI 中启动的进程找不到 shell 里的命令

Linux GUI 应用不保证继承 `.bashrc`、`.profile` 等 shell 初始化文件中的 `PATH`。正式能力必须使用配置、manifest 或 Tauri resource path 的绝对路径，不能依赖交互 shell 环境。

## 12. 交接状态

- 主开发分支：`codex/drpa-next-platform`。
- 当前交接 PR：<https://github.com/EthanBird/drpa-client/pull/2>。
- Windows 稳定基线：`desktop-v1.0.0`，提交 `5e6c793`。
- Linux 第一优先级：让 Ubuntu 22.04 上的真实 Tauri Host + 开发 Python 跑通，并记录截图、终端日志和平台差异。
- Linux 第二优先级：把现有 `linux-x86_64` runtime builder 接入 CI，在修改打包布局前先取得可重复的 air-gap 运行时证据。

开始编码前请先阅读：

1. [`DEVELOPMENT.md`](DEVELOPMENT.md)
2. [`architecture/DRPA_NEXT.md`](architecture/DRPA_NEXT.md)
3. [`../offline/README.md`](../offline/README.md)
4. [`JUPYTER_INTEGRATION.md`](JUPYTER_INTEGRATION.md)
5. [`PORTABLE_RELEASE.md`](PORTABLE_RELEASE.md)

平台适配应落在小而明确的 adapter 或配置层。不要复制一套 Linux 业务页面，也不要用 `cfg` 把领域规则分叉成两套。
