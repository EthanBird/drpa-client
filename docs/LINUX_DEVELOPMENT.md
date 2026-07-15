# DRPA Next Linux 开发与移植交接

本文面向接手 Linux 桌面端的开发者，记录当前代码已经具备的跨平台基础、仍属于 Windows 的实现、可直接复现的 Linux 开发流程，以及把 Linux 从“可编译”推进到“可离线发布”所需的验收清单。

## 1. 当前结论

DRPA Next `1.0.0` 的正式发行物仍是 Windows x64 全量离线安装包。Linux x86_64 已完成第一轮代码与打包适配，可以由 Ubuntu 22.04 workflow 生成包含 sealed runtime 的 AppImage；在干净虚拟机、Wayland 和人工 GUI 验收完成前，它仍不是面向终端用户的正式 Release。

| 能力 | Linux 当前状态 | 证据或入口 |
| --- | --- | --- |
| React/Vite 浏览器开发 | 可用 | `npm run dev` 使用 `mockGateway` |
| Rust core | CI 覆盖 | `drpa-protocol`、`drpa-package`、`drpa-host` 在 Ubuntu 测试 |
| Tauri Host 编译 | CI 覆盖 | Ubuntu 安装 WebKitGTK 后执行 `cargo check -p drpa-desktop` |
| Tauri 桌面联调 | 可在本机进行 | `npm run tauri:dev`，需要图形会话和 WebKitGTK |
| Linux 数据目录与 `xdg-open` | 已有实现 | `app_local_data_dir()/workspace`、`open_directory_in_file_explorer` |
| Linux x86_64 runtime 规格 | 已声明 | `offline/runtime-spec.json` 的 `linux-x86_64` |
| Linux sealed runtime 构建器 | 已有通路 | `tools/offline/build_runtime_bundle.py` 支持 `linux-x86_64` |
| Linux sealed runtime CI | 已接入 | `.github/workflows/linux-desktop.yml` 的 Ubuntu 22.04 原生构建与 air-gap smoke |
| AppImage 最终布局 | 已实现，待跨发行版验收 | `tauri.linux.conf.json` 把 runtime 放入只读 resource，Host 使用 `resource_dir` 定位 |
| deb 最终布局 | 未实现 | 第一阶段只交付 AppImage |
| Linux 文件级热更新 | 未实现 | 当前命令、协议与独立 Worker 只接受 `windows-x86_64` |
| Linux GUI、浏览器、Jupyter 端到端 | CI 已覆盖首层 | AppImage 解包 bootstrap、Chrome/Jupyter smoke 与 Xvfb 启动；Wayland/人工验收待完成 |
| Linux 任务取消 | 已实现 | Python worker/Studio Kernel 独立 process group，`SIGTERM` 后超时 `SIGKILL` |

因此，Linux 后续工作重点已从“接通代码”转为“证明发行质量”：先取得专用 workflow 绿灯，再在 Ubuntu 22.04/24.04、X11/Wayland 和真实断网机器上完成验收。不要把一次 `cargo check`、单独生成 AppImage 或 Xvfb 启动当作正式交付完成。

## 2. 目标基线

第一阶段只处理 **Linux x86_64 + glibc**：

- 构建基线：Ubuntu 22.04；它能提供 WebKitGTK 4.1，也能降低 AppImage 的 glibc 最低版本。
- 验证系统：至少 Ubuntu 22.04 和 Ubuntu 24.04 的干净虚拟机。
- 首选发行格式：AppImage；deb 在 AppImage 验收完成且只读安装前缀确定后再加入。
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

原生打包与 CI GUI smoke 还会使用以下工具/运行库：

```bash
sudo apt install -y \
  dbus-x11 \
  libfuse2 \
  libgbm1 \
  libnss3 \
  xauth \
  xvfb
```

依赖边界：WebKitGTK/GTK、编译器和 `patchelf` 属于构建环境；用户侧 UI 依赖由 AppImage 打包。`xdg-utils` 用于请求桌面文件管理器，FUSE 不可用时可用 AppImage 的 extract-and-run 模式。Chrome for Testing、Python、uv 和 Python wheels 不来自系统 apt。

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

Linux Host 会在启动 Python worker 与 Studio Kernel 前调用 `CommandExt::process_group(0)`。任务运行期间 `RunProcessManager` 保存 `run_id → pgid`：取消时先把领域状态写为 `cancelled`，向整个进程组发送 `SIGTERM`，两秒后仍未退出则发送 `SIGKILL`。后台退出处理会检查 cancelled 状态，不再把它覆盖为 failed。`cargo test -p drpa-desktop` 包含真实 `sh + sleep` 进程组回归。

浏览器由 Python worker 派生并继承同一进程组，因此取消普通 RPAZ 会覆盖 Chrome 子进程。Agent 尚没有独立“取消当前 turn”协议，不属于这条任务取消链。

### 5.5 更新能力

当前 `apply_windows_update`、`drpa-updater`、更新清单 target 和重启确认协议均为 Windows 实现。Host 通过 `get_platform_capabilities` 返回 `supportsWindowsUpdates=false`，Linux 设置页不会渲染 Windows 更新入口。Linux 第一版采用完整 AppImage 替换；不要复用 Windows `.drpa-update`。

## 6. 构建 Linux sealed runtime

构建器已经支持 `linux-x86_64`，但必须在原生 Linux x86_64、Python 3.11.9 上执行：

```bash
./.venv/bin/python -m pip install 'uv==0.11.28'
./.venv/bin/python tools/offline/validate_requirements.py offline/requirements/runtime.txt
./.venv/bin/python tools/offline/build_runtime_bundle.py \
  --platform linux-x86_64 \
  --work-dir "$PWD/offline-build/linux-x86_64"
```

构建阶段需要网络，用于下载受控 Python、精确 wheels 和固定 Chrome for Testing。`offline/requirements/runtime.txt` 是跨平台精确版本集合，Windows-only 项使用 environment marker；Linux 构建必须在 CPython 3.11.9 x86_64 原生环境执行 `pip download --only-binary=:all:`，不能使用旧 `wheelhouse/linux-x86_64/` 兼容目录。构建器随后会在空 uv cache、禁止联网的条件下：

1. 创建全新环境；
2. 验证 adapter 与依赖；
3. 用 DrissionPage 启动内置 Chrome 并访问本地 HTML；
4. 启动真实 Jupyter Kernel 连续执行两个单元；
5. 生成 `wheelhouse-lock.json`（实际文件名、版本、大小、SHA-256）、`manifest.json`、文件库存、归档与归档校验文件。

Linux wheel 中需要重点关注的原生/ABI 包包括 `debugpy`、`lxml`、`numpy`、`pandas`、`psutil`、`pyzmq`、`rpds-py` 和 `tornado`；其余纯 Python wheel 同样进入锁和散列清单。最终布局验证器会拒绝 `win_amd64`、`macosx`、`musllinux` wheel 混入 glibc 包，并复核所有 wheel 的大小和散列。

输出目录位于：

```text
offline-build/linux-x86_64/out/
```

把归档解压后，将内部 `drpa-runtime-...-linux-x86_64/` 目录作为 `DRPA_RUNTIME_ROOT` 即可进行 Host 联调。也可运行：

```bash
python tools/linux/verify_runtime_layout.py --runtime-root /path/to/runtime
```

该检查覆盖平台、Python 版本一致性、安全路径、wheel 散列以及 Python/uv/Chrome 可执行位。

## 7. Tauri Linux 包

AppImage 构建使用 `apps/desktop/src-tauri/tauri.linux.conf.json`。它把构建阶段临时目录 `resources/linux/runtime/` 映射到 AppImage 的 `$RESOURCES/runtime/`；该临时目录被 `.gitignore` 排除，禁止把几百 MB 二进制提交进 Git。

本地完整构建顺序：

```bash
runtime_stage="$(find offline-build/linux-x86_64 -maxdepth 1 -type d -name 'drpa-runtime-*-linux-x86_64' -print -quit)"
rm -rf apps/desktop/src-tauri/resources/linux/runtime
mkdir -p apps/desktop/src-tauri/resources/linux/runtime
cp -a "$runtime_stage/." apps/desktop/src-tauri/resources/linux/runtime/
python tools/linux/verify_runtime_layout.py --runtime-root apps/desktop/src-tauri/resources/linux/runtime
npm run tauri:build --workspace @drpa/desktop -- --bundles appimage
```

Host 启动时把 `app.path().resource_dir()/runtime` 放在运行时候选列表中，优先级低于显式 `DRPA_RUNTIME_ROOT`、高于应用旁外置 runtime。AppImage 内 runtime 保持只读；`bootstrap_runtime.py` 在 XDG workspace 的 `runtime-environment/environment` 创建可写 venv。

完整 AppImage 可直接联调：

```bash
chmod +x path/to/DRPA-Next_1.0.0_amd64.AppImage
DRPA_DATA_DIR="$PWD/.drpa-appimage-data" \
  path/to/DRPA-Next_1.0.0_amd64.AppImage
```

CI 会用 `--appimage-extract` 找到最终 `runtime/manifest.json`，对解包后的真实文件再次运行布局检查和离线 bootstrap，再用 `APPIMAGE_EXTRACT_AND_RUN=1 + Xvfb` 确认 GUI 不早退。deb 尚未设计，不能直接把 AppImage resource 路径假设搬到 `/usr/bin`。

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
./.venv/bin/python -m unittest discover -s tools/linux/tests -v
./.venv/bin/python -m compileall -q tools/offline tools/linux offline/bootstrap runtime/python/src
./.venv/bin/python tools/offline/validate_requirements.py offline/requirements/runtime.txt
./.venv/bin/python tools/release/check_version_consistency.py --expected 1.0.0
```

`tools/windows/tests` 在 Linux CI 运行的是更新包的纯 Python 格式与策略测试，不代表 Windows Worker 已在 Linux 上变成可用能力。

## 9. 推荐实施顺序

### 里程碑 A：源码模式可用（代码已具备，需人工复核）

- 在 Ubuntu 22.04 图形会话启动 `npm run tauri:dev`。
- 工作区、知识文档、Studio 文件操作和 `xdg-open` 正常。
- 使用开发 Python 运行普通 RPAZ、Bing 示例和两个 Notebook 单元。
- 设置页隐藏或禁用 Windows-only 能力，并显示准确平台状态。

### 里程碑 B：sealed runtime 可复现（已进入专用 workflow）

- 在 Ubuntu 22.04 构建 `linux-x86_64` runtime 与 wheelhouse lock。
- 在原生 Ubuntu runner 完成 air-gap bootstrap、DrissionPage 和 Jupyter smoke。
- 发布 runtime artifact，但暂不发布桌面端。
- 修正所有写死“Windows 运行时”的诊断文案。

### 里程碑 C：最终包布局（AppImage 已实现，deb 待定）

- AppImage 使用 Tauri resource path 定位 runtime；不要改回外置同目录猜测。
- deb 只有在明确只读安装前缀与升级合同后再加入。
- 最终包不依赖系统 Python、系统 Chrome、npm 或网络。
- 数据目录遵守 XDG，应用移动或升级不损坏用户数据。

### 里程碑 D：原生验收与发布（当前阻塞项）

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

专用 workflow 生成的 AppImage 包含完整 runtime；自行运行普通 `tauri build` 而没有先 stage runtime 的结果不属于完整离线产品包。

### GUI 中启动的进程找不到 shell 里的命令

Linux GUI 应用不保证继承 `.bashrc`、`.profile` 等 shell 初始化文件中的 `PATH`。正式能力必须使用配置、manifest 或 Tauri resource path 的绝对路径，不能依赖交互 shell 环境。

## 12. 交接状态

- 主开发分支：`codex/drpa-next-platform`。
- 当前交接 PR：<https://github.com/EthanBird/drpa-client/pull/2>。
- Windows 稳定基线：`desktop-v1.0.0`，提交 `5e6c793`。
- Linux 第一优先级：取得 `Build Linux x86_64 offline desktop` workflow 全绿，处理真实 Rust/Clippy/Tauri/AppImage 日志。
- Linux 第二优先级：在 Ubuntu 22.04/24.04 干净虚拟机分别完成 X11/Wayland、断网首次启动、RPAZ、Notebook、Agent 工具、取消进程树和中文路径人工验收。

开始编码前请先阅读：

1. [`DEVELOPMENT.md`](DEVELOPMENT.md)
2. [`architecture/DRPA_NEXT.md`](architecture/DRPA_NEXT.md)
3. [`../offline/README.md`](../offline/README.md)
4. [`JUPYTER_INTEGRATION.md`](JUPYTER_INTEGRATION.md)
5. [`PORTABLE_RELEASE.md`](PORTABLE_RELEASE.md)

平台适配应落在小而明确的 adapter 或配置层。不要复制一套 Linux 业务页面，也不要用 `cfg` 把领域规则分叉成两套。
