# UOS Desktop 20 兼容包构建、打包与验证手册

本文记录 DRPA Next `1.0.0` 的 UOS Desktop 20 Professional（eagle）x86_64 专用 Debian 包如何生成、为什么采用私有 ELF 运行层、哪些依赖必须随包携带、哪些依赖必须由目标机提供，以及发布前必须通过的自动化与人工验收。目标是让后续维护者能够复现当前产物，而不是只复制一条 `dpkg-deb` 命令。

当前正式产物：

- 文件：`drpa-next-1.0.0-linux-x86_64-uos20.deb`
- Debian 包名：`drpa-next`
- Debian 版本：`2.0.4-1+uos20.3`
- 架构：`amd64`
- 固定安装根：`/opt/drpa-next-uos20`
- 最低系统用户态：glibc 2.28
- 构建运行层：Ubuntu 22.04 / glibc 2.35
- Release：<https://github.com/EthanBird/drpa-client/releases/tag/desktop-v2.0.4>
- 构建与散列：以 Release 中 `uos20.deb.sha256` 和 `uos20-deb-manifest.json` 为准

`1.0.0-2+uos20.1` 虽能安装且 Host/NetworkProcess 持续运行，但在 Fantasy II-M（PCI `1ec8:9810`）上 WebKitWebProcess 因缺少可加载的 swrast 驱动与 `EGL_NOT_INITIALIZED` 退出，造成永久白屏；旧门禁只检查进程 20 秒未退出，无法发现该问题，已禁止继续作为可用版本。

`1.0.0-2+uos20.2` 修复了 EGL/swrast 白屏，React、IPC 与 WebKitWebProcess 也都通过，但固定 Deepin 20.8 镜像的截图完全没有文字。旧像素门禁只检查整图颜色数与灰度方差，图表和卡片足以让它误判通过；该版本同样被 `uos20.3` 取代。

真实 UOS 的系统字体已经由用户确认正常；`uos20.3` 不把字体重复塞进 deb。Debian 10 测试镜像能匹配 Noto CJK，承担可读文字截图门禁；固定 Deepin 20.8 镜像虽然请求安装字体包，但其 fontconfig 仍没有可用字体，因此只验收非白屏布局、WebKit 进程和真实输入后的页面响应，并明确记录 `font_available=0`。后续实体机复核发现只切换到 XIM 仍会连接 DDE/Fcitx，并且系统 `GTK_MODULES`、AT-SPI 或 XInput2 也可在控件聚焦时进入私有 Ubuntu GTK。当前启动器因此采用完整 GTK 模块隔离和 core input，而不是继续把 XIM 当作最终修复。

## 1. 目标环境与兼容性声明

初始目标机器为：

| 项目 | 目标值 |
| --- | --- |
| 发行版 | UOS Desktop 20 Professional（eagle） |
| 架构 | x86_64 |
| 内核 | 4.19.0-amd64-desktop |
| 系统 glibc | 2.28.31-deepin1 |
| 系统 GCC/C++ 时代 | GCC 8.3 |
| 已知缺包 | `libwebkit2gtk-4.1-0:amd64` |

自动化门禁分别使用 Debian 10 `debian:10-slim` 与 Deepin 20.8 `linuxdeepin/apricot:v20.8-compatible` 提供真实 glibc 2.28/同代桌面用户态。Debian 基线明确不安装系统 `libwebkit2gtk-4.1-0` 或 Mesa DRI；两层都会验证用户态 ABI、离线 Python/Jupyter、Chrome、React 挂载、Tauri IPC 与实际非白屏截图。Docker 仍共享 GitHub Runner 的宿主内核和虚拟 X11，不能表述为“已经在 UOS 4.19/Fantasy II-M 实体机完整验证”；每次改变 Chrome、WebKitGTK、glibc 或软件渲染闭包后，仍须在真实 UOS 20 / kernel 4.19 / DDE 与实际显卡上人工回归。

兼容范围只包含 Linux x86_64 + glibc。ARM、musl、32 位 x86、非 Debian 包管理系统不属于此产物的承诺范围。

## 2. 为什么普通 AppImage/deb 不能直接用于 UOS 20

普通 Linux 包在 Ubuntu 22.04 原生构建，其桌面 Host 和桌面运行库包含以下 ABI 要求：

- Rust/Tauri Host 可能引用 `GLIBC_2.34`；
- WebKitGTK/JavaScriptCoreGTK 可能引用 `GLIBC_2.35`；
- C++ 组件需要 `GLIBCXX_3.4.30` 和相应 `CXXABI`；
- 现代 WebKitGTK 的 GBM 路径需要 `gbm_bo_create_with_modifiers2`；
- 现代通用 libdrm 路径需要 `drmGetFormatModifierName`。

UOS 20 的 glibc 2.28、GCC 8 时代 libstdc++、GBM 和 libdrm 无法满足这些符号。删除 Debian `Depends`、复制一个 `libwebkit2gtk` 文件或伪造 control 版本都不会改变 ELF 的真实符号需求，最终只会把安装错误推迟成启动时的 `version not found` 或 `undefined symbol`。

当前方案不是在 UOS 上重新编译整个 Rust/WebKitGTK 技术栈，而是从已经通过验证的 Ubuntu 22.04 AppImage AppDir 派生固定根 deb，并给应用拥有的 ELF 配置一套私有 glibc/C++/用户态图形运行层。

## 3. 总体流水线

```text
精确 runtime 依赖锁
        │
        ▼
Ubuntu 22.04 构建 sealed CPython 3.11/Jupyter/Chrome
        │ air-gap bootstrap + Jupyter/Chrome smoke
        ▼
Tauri 构建 runtime-complete AppImage
        │ 解包后再次验证 runtime 与 X11 GUI
        ▼
验证后的 AppDir
        │
        ├── tools/linux/build_bundled_deb.py  ──► 现代 deb
        │
        └── tools/linux/build_uos20_deb.py
                │ 私有 loader/libc/C++/NSS/GBM/libdrm
                │ 递归 DT_NEEDED 闭包
                │ ELF interpreter + DT_RPATH
                ▼
              UOS 20 deb
                │
                ├── 静态解包与 ELF/manifest 校验
                └── Debian 10/glibc 2.28 容器真实安装与运行
```

UOS 包必须从同一轮已经验证的 AppDir 派生。不要绕过 AppImage runtime 验证，直接拿 `cargo build` 的单个 Host 二进制组装 deb；那样会漏掉 WebKit helper、GTK/GStreamer、sealed Python、wheelhouse 或 Chrome。

## 4. 代码与职责边界

| 文件 | 职责 |
| --- | --- |
| `.github/workflows/linux-desktop.yml` | Ubuntu 22.04 原生构建、磁盘回收、glibc 2.28 容器门禁、9 项资产发布 |
| `tools/linux/build_uos20_deb.py` | 固定根布局、私有运行层、依赖闭包、ELF 修补、control、deb 生成 |
| `tools/linux/verify_uos20_deb.py` | 包元数据、禁止依赖、私有符号、ELF interpreter/RPATH、launcher、runtime 校验 |
| `tools/linux/uos20-smoke.Dockerfile` | Debian 10/glibc 2.28 最低用户态测试镜像 |
| `tools/linux/uos20_container_smoke.sh` | 安装后 loader、Python/Jupyter、Chrome、X11、卸载数据验证 |
| `tools/linux/tests/test_uos20_package_policy.py` | 构建与验证策略的单元测试 |
| `tools/linux/build_bundled_deb.py` | 普通现代 deb 与 UOS 包共享的 AppDir/桌面元数据基础函数 |
| `tools/offline/build_runtime_bundle.py` | sealed Python、wheelhouse、uv、Chrome runtime 构建 |

修改包策略时，必须同步检查构建器、验证器、策略测试、workflow、Release notes 和本文件，不能只改其中一处。

## 5. 最终安装布局

```text
/
├── usr/bin/drpa-next                         # UOS launcher
├── usr/bin/drpa-desktop -> drpa-next
├── usr/share/applications/...                # desktop entry
├── usr/share/icons/...                       # 图标
├── usr/share/doc/drpa-next/
│   ├── README.UOS20
│   └── *-copyright                           # glibc/libgcc/libstdc++ 许可证
└── opt/drpa-next-uos20/
    ├── uos-runtime/
    │   ├── ld-linux-x86-64.so.2              # 私有动态加载器
    │   ├── libc.so.6
    │   ├── libstdc++.so.6
    │   ├── libgcc_s.so.1
    │   ├── libsoftokn3.so + .chk             # NSS dlopen 模块
    │   ├── libgbm.so.1
    │   ├── libdrm.so.2
    │   └── ...                                # 递归用户态依赖闭包
    ├── uos-runtime-manifest.json              # 来源、大小、散列、ELF 库存
    └── usr/
        ├── bin/drpa-desktop
        ├── lib/.../WebKitNetworkProcess
        ├── lib/.../WebKitWebProcess
        ├── lib/DRPA Next/runtime/
        │   ├── manifest.json
        │   ├── bootstrap_runtime.py
        │   ├── python/
        │   ├── wheelhouse/
        │   └── browser/
        └── share/...
```

应用文件是包管理器管理的只读内容。项目、设置、日志、生成环境和运行产物继续写入 Host 的 XDG 本地数据目录；卸载 deb 不得删除这些用户数据。测试时可用 `DRPA_DATA_DIR` 指定隔离目录。

现代 deb 和 UOS deb 的 Debian 包名都为 `drpa-next`，因此两者不能并存；安装其中一个会升级或替换另一个。维护者必须通过版本和文件名区分目标系统，UOS 用户只能选择文件名包含 `uos20` 的资产。

## 6. 依赖分层

### 6.1 随包携带

- glibc 2.35 动态加载器及 libc、libm、libdl、libpthread、librt、libresolv 等运行组件；
- glibc NSS 模块，如 `libnss_files.so.2`、`libnss_dns.so.2`；
- libstdc++、libgcc；
- WebKitGTK 4.1、JavaScriptCoreGTK、GTK 3、GStreamer、Soup、WebKit helper；
- NSS/NSPR 的运行库及通过 `dlopen` 加载的 `libsoftokn3`、`libfreebl3`、`libnssdbm3`、`libnssckbi` 和匹配 `.chk`；
- 满足现代 WebKitGTK 符号要求的 `libgbm.so.1` 和通用 `libdrm.so.2`；
- 通过 `DT_NEEDED` 递归发现的 X11/XCB、ALSA、字体、Fribidi 等非驱动用户态库；
- sealed CPython 3.11.9、uv、完整离线 wheelhouse、Jupyter、DrissionPage、Chrome for Testing。

### 6.2 由系统提供

当前 control 只声明：

```text
libc6 (>= 2.28), xdg-utils
```

其中 `libc6` 表示最低目标用户态与 Debian 包管理基线；应用 ELF 实际使用包内 loader/libc。`xdg-utils` 用于从 GUI 打开工作区与产物目录。EGL/GL/GLX/GLdispatch、Mesa EGL vendor、swrast/kms_swrast 与 llvmpipe 现在作为一套隔离、固定版本的软件渲染闭包随包携带；不复制任何硬件 DRI 模块，因此不会把 Ubuntu 厂商驱动绑定到 UOS 4.19 内核。

目标系统只负责内核、X11 server/DDE 会话和 `xdg-open` 集成。软件渲染会降低重型 WebGL 的性能，但 DRPA 主界面与 Monaco/Notebook 的可靠显示优先于不可用的硬件 EGL 路径。

### 6.3 仅构建环境需要

`build-essential`、`dpkg-dev`、`fakeroot`、`patchelf`、WebKitGTK 4.1 开发包、Tauri 编译依赖、Node.js、Rust 和构建 Python 都不应出现在最终目标机依赖中。

## 7. 私有 ELF 运行层

### 7.1 固定动态加载器

构建器识别所有 x86_64 动态 ELF，并对具有 interpreter 的可执行文件执行等价操作：

```bash
patchelf \
  --set-interpreter /opt/drpa-next-uos20/uos-runtime/ld-linux-x86-64.so.2 \
  <elf>
```

这保证桌面 Host、WebKit helper、Python、uv 和 Chrome 的动态入口不会先交给 UOS 的 glibc 2.28 loader。当前发布 manifest 记录 195 个已修补 ELF，其中 12 个包含动态解释器。

### 7.2 必须使用传递型 `DT_RPATH`

每个动态 ELF 都写入私有绝对搜索目录，并使用 `patchelf --force-rpath` 生成 `DT_RPATH`：

```text
/opt/drpa-next-uos20/uos-runtime
/opt/drpa-next-uos20/usr/lib
/opt/drpa-next-uos20/usr/lib/x86_64-linux-gnu
```

这里不能退化成 `DT_RUNPATH`。WebKit、Chrome 和 Python 原生扩展拥有多层依赖，`DT_RPATH` 的传递行为能让子依赖继续找到私有 libc/C++ 与用户态闭包；验证器会拒绝出现 `RUNPATH` 或缺少任一私有路径的应用 ELF。

### 7.3 不使用全局 `LD_LIBRARY_PATH`

launcher 不导出 `LD_LIBRARY_PATH`。否则从应用启动的 `/usr/bin/xdg-open`、文件管理器、shell 或其他系统程序也会继承私有 glibc 2.35，进而和 UOS 自身的 2.28 用户态混用。私有解释器与每个应用 ELF 的 RPATH 已经足以完成隔离，系统子进程继续使用系统 loader 和系统库。

### 7.4 递归闭包与动态加载模块

构建器从 AppDir 和私有运行层的 ELF 出发，读取 `patchelf --print-needed`，递归复制尚未存在且不属于显卡驱动边界的 SONAME。仅扫描 `DT_NEEDED` 仍不完整：NSS 会在运行时通过 `dlopen` 查找加密模块，因此 `libsoftokn3`、`libfreebl3`、`libnssdbm3`、信任模块和 `.chk` 文件必须显式列入 `SYSTEM_RUNTIME_FILES`。

Ubuntu 上 NSS 文件可能位于普通 multiarch 目录、`nss/` 子目录或由 p11-kit 提供。构建器为这些文件维护多个候选路径，复制后统一落到私有运行层根目录，并记录真实来源与 SHA-256。

## 8. WebKitGTK 与图形兼容处理

### 8.1 WebKit helper 相对路径

Ubuntu 的生产版 WebKitGTK 使用编译期 `PKGLIBEXECDIR` 定位 Network/Web process。重定位 AppDir 后，helper 路径可能表现为相对于 `usr/` 的 `./lib/.../WebKitNetworkProcess`。`WEBKIT_EXEC_PATH` 只在 WebKit developer mode 生效，不能作为生产修复。

UOS launcher 因此在启动 Host 前执行：

```sh
cd "$APPDIR/usr"
exec /opt/drpa-next-uos20/usr/bin/drpa-desktop "$@"
```

验证器明确要求该工作目录切换，并拒绝重新引入无效的 `WEBKIT_EXEC_PATH`。

### 8.2 GBM 与 libdrm

UOS 时代的系统 GBM 和 libdrm 缺少现代 WebKitGTK 使用的符号。构建器把 Ubuntu 22.04 的兼容 `libgbm.so.1` 与通用 `libdrm.so.2` 放入私有层，验证器同时检查：

- `gbm_bo_create_with_modifiers2`；
- `drmGetFormatModifierName`。

launcher 固定 `LIBGL_ALWAYS_SOFTWARE=1`，把 `LIBGL_DRIVERS_PATH` 与 `__EGL_VENDOR_LIBRARY_FILENAMES` 指向包内 Mesa，并设置 `GALLIUM_DRIVER=llvmpipe`、禁用 DMABUF/GBM 与 WebKit compositing 路径。包中只含 swrast/kms_swrast，不含厂商硬件 DRI；这能避开 Fantasy II-M 的 EGL 初始化失败，但仍不替代实体机图形测试。

launcher 同时设置 `DRPA_UI_REDUCED_EFFECTS=1`。Host 通过平台能力协议把该标记传给 React，前端据此关闭 `backdrop-filter`、连续动画和过渡，避免 llvmpipe 在弹窗、菜单与页面交互时反复执行整窗软件重绘。普通 Windows、现代 Linux 与 macOS 启动路径不设置该标记，保留完整视觉效果。

### 8.3 X11 基线

当前 launcher 固定 `GDK_BACKEND=x11`，自动化用 Xvfb 验证 X11 启动。Wayland/DDE 混合环境尚未成为发布硬门禁；如果未来开放 Wayland backend，必须保留 X11 回归并新增真实 Wayland 会话测试。

### 8.4 GTK 输入与动态模块隔离

UOS 包包含 Ubuntu 22.04 的私有 GTK/WebKitGTK，但目标桌面可能运行 Deepin 定制 IBus/Fcitx、AT-SPI 和 XInput2。第一次聚焦 `<input>`/`textarea`、打开下拉框或创建辅助对象时才会加载输入法/辅助功能模块；混合两代 GTK/GLib 用户态后可能阻塞 WebKit 输入上下文，表现为窗口仍能拖动、页面内按钮、输入框和 Tauri IPC 入口却全部停滞。

launcher 因此设置：

```sh
export GTK_PATH="$APPDIR/usr/lib/x86_64-linux-gnu/gtk-3.0"
unset GTK_MODULES GTK3_MODULES
export NO_AT_BRIDGE=1
export GTK_IM_MODULE=gtk-im-context-simple
export GDK_CORE_DEVICE_EVENTS=1
```

`GTK_PATH` 不再包含 `/usr/lib/.../gtk-3.0`；会话注入的 `GTK_MODULES` / `GTK3_MODULES` 被清除，AT-SPI bridge 不跨用户态连接，GTK 使用内置 simple context，GDK 只消费 X11 core events。默认模式优先保证全部业务控件和 IPC 持续可用；复杂文字可通过剪贴板输入。若未来开放 `ibus`、`fcitx` 或 XIM 原生预编辑，必须把匹配私有 GTK 版本的模块与服务协议纳入包内闭包，并在真实 UOS/DDE 会话重新通过输入、下拉框、弹窗、工作区创建和 IPC 门禁。

## 9. 可复现构建

### 9.1 构建机要求

正式包只能在 `.github/workflows/linux-desktop.yml` 指定的 `ubuntu-22.04` 原生 x86_64 Runner 组装。构建器会执行：

```bash
getconf GNU_LIBC_VERSION
```

结果必须严格为 `glibc 2.35`。`--system-root` 参数用于测试 fixture 或受控 sysroot，不是允许在任意新发行版上悄悄生成正式 UOS 包的后门。

关键 apt 构建依赖包括：

```text
build-essential dbus-x11 dpkg-dev fakeroot file
libayatana-appindicator3-dev libegl1 libegl-mesa0 libfuse2 libgbm1 libgl1
libgl1-mesa-dri libglx-mesa0 libopengl0
libnss3 librsvg2-dev libssl-dev libwebkit2gtk-4.1-dev libxdo-dev
patchelf xauth xdg-utils xvfb
```

此外需要 Node.js 24、Rust stable、CPython 3.11.9 和精确版本的 runtime builder。完整版本与命令以 workflow、`package-lock.json`、`Cargo.lock`、`offline/runtime-spec.json` 和 `offline/requirements/runtime.txt` 为准。

### 9.2 从验证后的 AppDir 生成 deb

若已有 runtime-complete AppImage，可先在独立目录解包：

```bash
mkdir -p /tmp/drpa-uos-build/appimage-extract
cd /tmp/drpa-uos-build/appimage-extract
/absolute/path/drpa-next-1.0.0-linux-x86_64.AppImage --appimage-extract
```

随后回到仓库根目录执行：

```bash
python tools/linux/build_uos20_deb.py \
  --appdir /tmp/drpa-uos-build/appimage-extract/squashfs-root \
  --package-version '2.0.4-1+uos20.3' \
  --output /tmp/drpa-next-1.0.0-linux-x86_64-uos20.deb \
  --work-dir /tmp/drpa-uos-build/package-work
```

构建步骤按顺序为：

1. 验证输入 AppDir 具备完整桌面布局；
2. 复制 AppDir 到 `/opt/drpa-next-uos20` 对应的包根；
3. 给应用 ELF 写入私有 interpreter 与传递型 RPATH；
4. 复制固定 glibc/C++/NSS、GLVND/Mesa EGL、swrast/kms_swrast，并递归补齐 llvmpipe/LLVM 等 `DT_NEEDED`；
5. 生成隔离系统 GTK 模块、固定 simple input/core events 的软件渲染 launcher、desktop entry、图标和文档；
6. 写入包内 `uos-runtime-manifest.json`；
7. 生成 Debian control 和 `Installed-Size`；
8. 使用 `dpkg-deb --root-owner-group -Zxz -z6` 压缩产物。

不要为了缩短构建时间删除静态验证或容器测试。若 GitHub Runner 空间不足，应像当前 workflow 一样先把最终资产移动到 Runner 临时目录的 `linux-assets`，再删除 Cargo target、runtime staging、AppImage 解包目录和 deb 中间目录，并执行 Docker/apt 缓存清理。

## 10. 静态验证

生成 deb 后执行：

```bash
python tools/linux/verify_uos20_deb.py \
  --deb /tmp/drpa-next-1.0.0-linux-x86_64-uos20.deb \
  --expected-version '2.0.4-1+uos20.3' \
  --extract-root /tmp/drpa-uos-build/verify-root \
  --manifest-output /tmp/drpa-next-1.0.0-linux-x86_64-uos20-deb-manifest.json
```

验证器覆盖：

- 包名、版本、`amd64` 架构和 `libc6 (>= 2.28)`；
- `Depends` 不得出现系统 WebKitGTK/JavaScriptCoreGTK/GTK、libstdc++6、`libgcc-s1`、`libgbm1`、`libdrm2`、`libegl1` 或 `libgl1`；
- 私有 loader、glibc、C++、NSS、GBM、libdrm、X11、音频和字体关键文件存在；
- 私有 GBM/libdrm 具备必需符号，GLVND/Mesa EGL、swrast/kms_swrast 与 vendor manifest 完整；
- launcher 把 `GTK_PATH` 限定到包内，清除 `GTK_MODULES` / `GTK3_MODULES`，设置 `NO_AT_BRIDGE=1`、`GTK_IM_MODULE=gtk-im-context-simple` 与 `GDK_CORE_DEVICE_EVENTS=1`；
- 所有应用动态 ELF 的 RPATH 完整且为 `DT_RPATH`；
- 所有带 interpreter 的应用 ELF 指向固定私有 loader；
- 实际 ELF 数量与包内 provenance manifest 一致；
- `/usr/bin/drpa-next` 可执行、不导出 `LD_LIBRARY_PATH`、切换到 `$APPDIR/usr` 且不使用 `WEBKIT_EXEC_PATH`；
- 包内只有一份 sealed runtime，manifest、wheel 散列、平台、Python/uv/Chrome 可执行位均正确。

发布 manifest 是对最终 deb 的机器可读摘要；包内 provenance manifest 进一步记录每个私有运行库的来源、字节数、SHA-256、修补 ELF 清单和许可证文件。

## 11. glibc 2.28 与可视 UI 容器门禁

静态检查无法证明应用真的显示内容。workflow 在回收构建磁盘后，先用 `tools/linux/uos20-smoke.Dockerfile` 创建 Debian 10 测试镜像，并在启动前屏蔽截图工具间接带入的系统 Mesa DRI，再用 `tools/linux/uos20-deepin-smoke.Dockerfile` 在 Deepin 20.8 同代用户态重复真实安装与可视测试：

```bash
docker build \
  --build-arg DEB_FILENAME=drpa-next-1.0.0-linux-x86_64-uos20.deb \
  --file tools/linux/uos20-smoke.Dockerfile \
  --tag drpa-next-uos20-smoke \
  <包含 deb 与 uos20_container_smoke.sh 的临时 context>

docker run --rm --volume /tmp/uos-diagnostics:/diagnostics drpa-next-uos20-smoke
```

容器测试必须全部满足：

1. `getconf GNU_LIBC_VERSION` 精确返回 `glibc 2.28`；
2. 系统没有安装 `libwebkit2gtk-4.1-0`；
3. 私有 loader 能 `--verify` 和 `--list` 桌面 Host；
4. loader 列表中的 libc、libstdc++、X11、Fribidi、GBM、libdrm 与 EGL 来自 `/opt/drpa-next-uos20/uos-runtime`，包内存在 GLVND/Mesa vendor 和 swrast/kms_swrast；
5. 记录测试镜像中 `fc-match 'sans-serif:lang=zh-cn'` 的结果和 `font_available`；字体由目标桌面系统提供，不属于 deb 私有闭包，Debian 10 必须命中字体，固定 Deepin 镜像允许明确记录无字体；
6. sealed Python 通过 `ctypes` 观察到私有 glibc 2.35；
7. 在 `PIP_NO_INDEX=1`、`UV_OFFLINE=1`、禁止下载 Python 的条件下创建全新环境；
8. 新环境导入 `debugpy`、`drpa_runner`、DrissionPage、ipykernel、jupyter_client、lxml、nbformat、NumPy、Pandas、psutil、rpds、tornado、ZMQ；
9. 内置 Chrome 以普通用户完成 headless DOM 测试；
10. 前端成功取得 workspace snapshot，等待两次 `requestAnimationFrame` 后通过 Tauri IPC 写入 `reactMounted=true` 与 `ipcRoundTrip=true` 就绪标记；
11. `WebKitWebProcess` 在标记产生后仍存活，日志不得包含 `EGL_NOT_INITIALIZED`、无法创建 EGL display、swrast 加载失败或 `Aborting`；
12. Host 写出 Linux Dify2API sidecar 后，以普通用户启动真实服务并要求 `/healthz` 返回 `service=dify2api`；日志和健康响应作为诊断资产保存；
13. 启动前注入 DDE 风格的 Fcitx、系统 `GTK_PATH`、`GTK_MODULES=gail:atk-bridge` 与 `GTK3_MODULES=atk-bridge`，启动后读取 Host `/proc/.../environ`，确认启动器已清理系统模块并启用 simple input/core events；
14. `xdotool` 用真实 X11 事件打开命令面板、物理点击输入框、逐键输入 `drpa-input-smoke`、关闭面板并点击侧栏按钮；前端必须在后续点击 300ms 后仍能通过 Tauri IPC 写出 `nativeInputTyped=true`、`postInputClick=true` marker；
15. 捕获 1280×800 Xvfb 根窗口截图：始终要求至少 32 色；有字体时灰度标准差必须不低于 0.03，顶部 1100×100 文字区在 30% 灰度阈值下必须有至少 0.2% 深色像素、至少 5 个连通组件且最大组件不超过 500 像素；无字体的最小镜像要求灰度标准差不低于 0.01，并必须同时通过第 14 项原生输入 marker；
16. `dpkg --remove drpa-next` 后，测试数据哨兵仍存在。

Debian 10 镜像不会安装系统 WebKitGTK 4.1；截图工具间接带入的系统 Mesa DRI 目录会在启动应用前被移走，从而证明软件渲染闭包确实来自 deb。Deepin 20.8 镜像固定到不可变 SHA-256 digest，用于覆盖与 UOS 同代的发行版用户态；即使镜像本身带 Mesa，launcher 的私有 RPATH、DRI 路径和 EGL vendor manifest 仍会固定到包内闭包。两次测试始终上传 PNG、视觉指标、React/IPC marker、WebKit 进程树、X11 window tree 和完整日志。

### 11.1 已撤销的 `uos20.2` 验证记录（2026-07-17）

- 发布提交：`75a6c4386aa58aa1a12bedf5ee7caa479cf12004`；Actions：[Linux workflow run 29551221688](https://github.com/EthanBird/drpa-client/actions/runs/29551221688)。
- UOS 包：`drpa-next-1.0.0-linux-x86_64-uos20.deb`，版本 `1.0.0-2+uos20.2`，Release 页面大小 324 MB，SHA-256 `ba7c6691cc172c1b74a1530d4b61dfca53c186ac396f18b67a8fd43f6feb4c88`。
- Debian 10 / glibc 2.28：`reactMounted=true`、`ipcRoundTrip=true`，Host、WebKitNetworkProcess 与 WebKitWebProcess 同时存活；截图为 2575 色，灰度标准差 0.0625469。
- Deepin 20.8：基础镜像固定为 `linuxdeepin/apricot:v20.8-compatible@sha256:be6ee56f055c4d3e3b1a77badaf7b42b3d0e70337f3ea3d203304d169ccefb78`；同样完成 React/IPC 与 WebKitWebProcess 检查，截图为 1825 色，灰度标准差 0.033566。
- Debian PNG 有可读拉丁文字；Deepin PNG 只有仪表盘卡片、图标和折线图，文字完全没有呈现。其顶部文字区深色像素比例为 `0`，所以该 run 只能证明 EGL 白屏已修复，不能证明输入框可交互。`uos20.3` 发布必须在有字体的 Debian 10 中通过严格文字门禁，并在 Debian/Deepin 两边都通过原生 X11 输入与后续页面点击 marker，才能在本节后追加有效验证记录。

### 11.2 `uos20.3` 最终验证记录（2026-07-17）

- 发布提交：`1932b0d6b4c86965381c6edbd166f67f8a05b88c`；Actions：[Linux workflow run 29564213354](https://github.com/EthanBird/drpa-client/actions/runs/29564213354)。
- UOS 包：`drpa-next-1.0.0-linux-x86_64-uos20.deb`，版本 `1.0.0-2+uos20.3`，340,090,572 字节，SHA-256 `ecefbe51b1375bab2f89e567c1eab121bcf5c3fba98da5f4ab30d8d4c380e59c`；manifest 的 `depends` 为 `libc6 (>= 2.28), xdg-utils`，`inputMethodMode` 为 `xim`。
- Debian 10 / glibc 2.28：`font_available=1`；最终截图 2103 色、灰度标准差 `0.0480815`、顶部深色像素比例 `0.00670909`、12 个文字组件，最大组件 165 像素。人工查看确认输入框显示 `drpa-input-smoke`，后续点击进入“开发工作室”，最终中文标题、说明和编辑器均可见。
- 固定 Deepin 20.8 用户态：`font_available=0`；截图仍呈现卡片、图表、输入框和 Studio 编辑器布局，590 色、灰度标准差 `0.0194148`。该图不用于证明文字渲染；真实 UOS 字体由用户实体机确认。
- 两个用户态的 `drpa-uos20-input-ready.json` 都包含 `nativeInputTyped=true`、`postInputClick=true`、`ipcRoundTrip=true`，证明真实 X11 输入发生后 WebKit timer、页面点击和 Tauri IPC 仍可继续执行；这直接覆盖“点击任意输入框后页面冻结但窗口仍能移动”的回归。

## 12. 发布资产与触发规则

Linux workflow 最终必须收集恰好 9 个资产：

```text
drpa-next-1.0.0-linux-x86_64.AppImage
drpa-next-1.0.0-linux-x86_64.AppImage.sha256
drpa-next-1.0.0-linux-x86_64.deb
drpa-next-1.0.0-linux-x86_64.deb.sha256
drpa-next-1.0.0-linux-x86_64-deb-manifest.json
drpa-next-1.0.0-linux-x86_64-uos20.deb
drpa-next-1.0.0-linux-x86_64-uos20.deb.sha256
drpa-next-1.0.0-linux-x86_64-uos20-deb-manifest.json
drpa-next-1.0.0-linux-x86_64-wheelhouse-lock.json
```

普通 push 只有在提交信息以 `release(linux):` 开头时才会上传并覆盖 Release 资产；也可手动运行 workflow 并设置 `publish=true`。文档提交不应伪装成发布提交，否则会无意义地重建数百 MB 产物。

发布前检查：

```bash
python -m unittest discover -s tools/linux/tests -v
python -m compileall -q tools/linux
git diff --check
```

发布后必须从 GitHub Release API 或页面重新核对文件名、大小、资产状态和 SHA-256，不能只看到 Actions 绿灯就宣布完成。

## 13. 已解决的典型故障

| 现象 | 根因 | 固化修复 |
| --- | --- | --- |
| `libfribidi.so.0` 等随机库缺失 | 只复制直接依赖，未覆盖传递闭包 | 递归解析全部非驱动 `DT_NEEDED` |
| Chrome NSS 错误、`-5925` | `libsoftokn3`/FreeBL 由 `dlopen` 加载，不出现在 `DT_NEEDED` | 显式携带 NSS 模块、信任模块和匹配 `.chk` |
| 找不到 `libnssckbi.so` | Ubuntu NSS/p11-kit 安装布局不同 | 为 multiarch、`nss/`、`pkcs11/` 维护候选源路径 |
| `gbm_bo_create_with_modifiers2` 未定义 | UOS 系统 GBM 太旧 | 私有携带并验证 `libgbm.so.1` |
| `drmGetFormatModifierName` 未定义 | UOS 系统通用 libdrm 太旧 | 私有携带并验证 `libdrm.so.2` |
| WebKit 查找 `././/lib/.../WebKitNetworkProcess` 失败 | 重定位后生产 `PKGLIBEXECDIR` 相对工作目录不正确 | launcher 在执行 Host 前 `cd "$APPDIR/usr"` |
| 设置 `WEBKIT_EXEC_PATH` 仍无效 | 该变量只在 WebKit developer mode 生效 | 删除该变量，按生产路径语义修复工作目录 |
| 系统工具加载私有 libc 后崩溃 | launcher 全局导出 `LD_LIBRARY_PATH` | 用 interpreter + 每 ELF RPATH 隔离，不污染系统子进程 |
| Docker 构建前磁盘只剩几十 MB | AppImage、Cargo target、两个 deb 工作树并存 | 先 stage 9 项最终资产，再清理构建树和缓存 |
| CI 构建成功但目标机仍失败 | 只做编译或静态检查，没有旧用户态运行 | 增加 Debian 10/glibc 2.28 真实安装、Chrome、X11 门禁 |
| 页面卡片和图表可见但没有任何文字 | 最小 Deepin 测试镜像没有可用系统字体，旧门禁又只看整图方差 | 不把目标系统字体重复塞进 deb；由 Debian 10 严格检查文字，Deepin 明确记录 `font_available=0` 并只验收布局、WebKit 和真实输入响应；实体 UOS 单独人工确认字体 |
| 聚焦输入框、打开弹窗或点击自定义控件后页面事件停滞 | 私有 Ubuntu GTK 仍通过 XIM、系统 `GTK_PATH`、`GTK_MODULES`、AT-SPI 或 XInput2 接入 UOS/DDE 用户态 | UOS launcher 只允许包内 GTK 模块，清除会话 modules，使用 simple input 与 X11 core events；门禁主动注入污染环境后验证 Host `/proc`、真实点击、键入和延迟 IPC |
| 连续访问多个业务页面后点击越来越慢或停滞 | 所有已访问 React 页面都以隐藏 DOM 常驻，旧 WebKit 的 `inert` 焦点树与 llvmpipe 模糊合成持续累积 | 只保留 BI、Studio、知识文档三个草稿型工作区；其余页面离开即释放；隐藏面不再使用 `inert`；UOS 启用低成本视觉配置 |
| Dify2API 显示已启用但 gateway 以退出码 1 停止 | 纯 executable 插件被错误绑定到 Python 初始化、用户数据分区使用 `noexec`、loopback 健康检查继承系统代理，或上次异常退出残留 sidecar 占用端口 | executable 服务跳过 Python 初始化；sidecar 从会话级 `0700` 缓存启动并绑定父进程生命周期；Host 关闭 loopback 代理，端口冲突时自动保存新端口，错误附带脱敏 stderr；Debian 10/Deepin 门禁运行真实 `/healthz` |

遇到新缺库时，不要立即把目标机的任意 `.so` 复制进包。先确认它属于普通用户态闭包还是显卡/内核 ABI 边界，再更新构建器、验证器和测试；对 `dlopen` 模块还要补完整的数据/校验伴随文件。

## 14. 目标机安装、诊断与卸载

安装前确认架构与 glibc：

```bash
dpkg --print-architecture
getconf GNU_LIBC_VERSION
```

期望为 `amd64` 和 `glibc 2.28` 或更高。安装：

```bash
sudo apt install ./drpa-next-1.0.0-linux-x86_64-uos20.deb
```

核对包信息：

```bash
dpkg-query -W -f='${Package} ${Version} ${Architecture}\n' drpa-next
dpkg-deb -f ./drpa-next-1.0.0-linux-x86_64-uos20.deb Depends
```

确认私有 loader 解析：

```bash
/opt/drpa-next-uos20/uos-runtime/ld-linux-x86-64.so.2 \
  --list /opt/drpa-next-uos20/usr/bin/drpa-desktop
```

启动日志排查可在终端运行：

```bash
G_MESSAGES_DEBUG=all /usr/bin/drpa-next
```

卸载：

```bash
sudo apt remove drpa-next
```

卸载不会主动删除 XDG 用户数据。若确需清理数据，应先在应用中确认数据目录并由用户显式备份/删除，不能把清理用户数据加入 package maintainer script。

## 15. UOS 20 实体机人工回归

发布后至少记录以下结果：

- 从文件管理器和终端各启动一次，窗口可移动、缩放、最小化、最大化和关闭；
- DDE/X11 会话正常，若系统使用 Wayland/XWayland 也记录实际 backend；
- 断网首次初始化运行环境，状态页显示 Python/Jupyter/Chrome 健康；
- 安装并运行 Bing 示例 RPAZ，浏览器能启动、日志和产物可见；
- Studio 连续运行两个 Notebook 单元，Kernel 状态保持；
- 打开工作区和产物目录，确认 `xdg-open` 未被私有 libc 污染；
- 取消任务后 Python/Jupyter/Chrome 进程树退出；
- 中文、空格与非 ASCII 项目路径；
- 普通用户运行，不依赖 root；
- 升级安装新 deb 后用户项目、设置、日志和运行记录保留；
- 卸载后 XDG 用户数据仍保留；
- 至少记录显卡型号、驱动/Mesa 版本、内核、会话类型和测试日志。

UOS 专用包从 `uos20.2` 起有意永久使用软件渲染，这是对已证实不兼容 GPU/EGL 路径的产品级兼容策略。人工回归要记录 CPU 占用、窗口交互和 Monaco/Notebook 性能；未来若恢复硬件加速，必须增加显卡白名单、可回退启动和同等像素级门禁，不能直接移除软件 fallback。

## 16. 后续维护约束

以下变化都要求重新执行完整 UOS 门禁和实体机回归：

- Rust toolchain、Tauri、WebKitGTK、GTK、GStreamer 或系统构建基线升级；
- CPython、uv、Jupyter、原生 wheel 或 Chrome for Testing 升级；
- 私有 glibc/libstdc++/libgcc、NSS、GBM、libdrm、GLVND/Mesa/swrast/LLVM 清单变化；
- AppDir 或 WebKit helper 目录变化；
- interpreter、RPATH、launcher、XDG 数据路径变化；
- Debian control 依赖、包名、安装根、版本或升级策略变化。

不要把 `ubuntu-latest` 作为 UOS 正式运行层来源；当前构建器故意要求 Ubuntu 22.04 的 glibc 2.35。未来若迁移构建基线，应先分析新 glibc 的最低内核/系统调用要求、C++ ABI、WebKit helper 和所有私有库来源，再引入新的包修订号与兼容矩阵。

项目已统一停止发布 `.drpa-update`。UOS 版本升级通过 `apt install ./新版-uos20.deb` 完成，包管理文件可替换，XDG 用户数据保持独立。
