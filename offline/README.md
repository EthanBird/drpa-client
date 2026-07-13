# DRPA sealed 离线运行时

sealed runtime 是平台专用、不可变、可验证的 Release 资产，不是源码目录、pip 缓存或可跨机器复制的 venv。当前只构建和发布 `windows-x86_64`。

## 运行时合同

```text
drpa-runtime-<version>-windows-x86_64/
├── python/                 可重定位 CPython 3.11.9
├── tools/uv.exe            固定版本的离线安装器
├── wheelhouse/             Windows cp311 完整 wheel closure
├── browser/                固定 Chrome for Testing
├── locks/runtime.txt       直接与传递依赖精确版本
├── bootstrap_runtime.py    幂等离线环境初始化
├── prepare-runtime.ps1
├── manifest.json           平台、精确可执行路径、大小和 SHA-256
└── SHA256SUMS
```

归档不包含已经创建的 venv。应用在最终安装位置使用 bundle 中的基础 Python、uv 和 wheelhouse 创建 `data/runtime-environment/environment`，以避免绝对路径和不可重定位环境。

`manifest.json` 中的 `pythonExecutable` 与 `browserExecutable` 是唯一定位来源。禁止递归搜索第一个 `python.exe`；这种做法会误选 `Lib\\venv\\scripts\\nt\\python.exe` 并触发 `No pyvenv.cfg file`。

## 依赖政策

版本来源：

- Python、uv、Chrome 与平台：`runtime-spec.json`
- Python 完整依赖集合：`requirements/runtime.txt`
- DRPA adapter：`../runtime/python/`

规则：

- 每一项直接和传递依赖都必须使用精确版本。
- 禁止 VCS URL、直接下载 URL、editable、额外 index 和未固定版本。
- Windows runtime 只接受与 CPython 3.11 / win_amd64 匹配的二进制 wheel。
- 离线机器永远不通过 pip 联网补依赖。
- 基础环境是经过策划的能力集合，不等于整个 PyPI；OCR、桌面自动化和本地 AI 等大型能力应拆成未来 runtime pack。

本地政策检查：

```bash
python tools/offline/validate_requirements.py offline/requirements/runtime.txt
python -m unittest discover -s tools/offline/tests -v
python -m compileall -q tools/offline offline/bootstrap runtime/python/src
```

## GitHub Actions 完整性证明

`.github/workflows/offline-runtime.yml` 和 Windows desktop release 会在原生 Windows runner 上：

1. 只下载目标平台二进制 wheels。
2. 构建 DRPA Python adapter wheel。
3. 安装受控 CPython，复制固定 uv，下载固定 Chrome。
4. 使用空 uv cache、`UV_OFFLINE=1`、`--offline --no-index --find-links` 创建全新环境。
5. 运行依赖一致性检查。
6. 用 DrissionPage 启动内置 Chrome 并访问本地 HTML。
7. 导入 runtime、浏览器、数据、Excel、`ipykernel`、`jupyter_client`、`zmq` 和 `nbformat`。
8. 启动真实 Jupyter/ZMQ Kernel，连续执行两个单元并验证状态和输出。
9. 生成逐文件散列、归档、上传 Actions artifact 并发布 prerelease。
10. desktop release 再在最终安装目录结构中执行一次 bootstrap 和关键 import。

任何缺 wheel、ABI 错误、路径错误、浏览器失败或锁文件不合法都会让 job 在发布前失败。

## 缺依赖处理 Runbook

离线机器出现 `ModuleNotFoundError`、DLL load failure 或缺少数据文件时，不要在用户机器上临时安装。按以下流程处理：

1. 保存完整错误日志、RPAZ manifest、runtime bundle version、Windows 版本和架构。
2. 判断依赖属于通用基础能力还是特定脚本包：
   - 多数包都会使用：加入 sealed baseline。
   - 只服务一个 RPAZ：由包携带锁定的 Windows wheels，或建立独立 runtime pack。
3. 在 `requirements/runtime.txt` 增加精确版本，并补齐所有传递依赖；若是 DLL/模型/浏览器数据，也必须进入清单。
4. 更新 import/行为 smoke test，确保不是“安装成功但运行失败”。
5. 运行本地政策检查并提交代码。
6. 触发原生 Windows sealed runtime workflow；不得用其他平台下载的 wheel 冒充 Windows 结果。
7. 确认 air-gap 初始化、关键 import、浏览器和 Jupyter smoke 全部通过。
8. 将新的完整 runtime、SHA-256 和依赖变更说明上传 GitHub Release，并让 desktop release 引用该版本。
9. 在一台真正断网、无系统 Python 的 Windows 测试机安装验证。
10. 在 `CHANGELOG.md` 记录新增依赖、体积影响、兼容性和回滚方式。

缺失依赖必须进入 GitHub 中可复现、可校验的 Release 资产；禁止只把文件发给单台机器而不更新锁、清单和构建流程。

## 发布与校验

当前 runtime Release：<https://github.com/EthanBird/drpa-client/releases/tag/offline-runtime-v0.2.0-dev-6>

传入离线网络前后均应验证配套 `.sha256`。PowerShell 示例：

```powershell
Get-FileHash .\drpa-runtime-*.zip -Algorithm SHA256
```

预览版尚未建立代码签名信任链，SHA-256 只能证明文件与发布记录一致。稳定版需要在散列之外增加签名清单、可信公钥轮换和撤销机制。
