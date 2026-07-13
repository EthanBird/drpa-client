# DRPA Client Windows 离线包使用说明

本压缩包已内置：

- uv 启动器：`.tools\uv\uv.exe`
- Python 3.11.9：`.tools\python\cpython-3.11.9-windows-x86_64-none\`
- 项目运行环境：`.venv\`
- 离线依赖库：`wheelhouse\windows-amd64\`

## 解压即用

1. 解压到任意目录，例如 `D:\drpa-client`
2. 双击运行：

```bat
scripts\run-drpa-windows.bat
```

脚本会：

1. 自动修正 `.venv\pyvenv.cfg` 中的 Python 路径（适配解压目录）
2. 用 `.venv\Scripts\python.exe -m drpa_client.app.main` 启动（绕过 uv trampoline 的绝对路径问题）
3. **不会**联网执行 `uv sync`

离线包已内置完整 PySide6/Qt 运行时（约 600+ MB，位于 `.venv\Lib\site-packages\PySide6\`），无需单独安装 Qt。

## 常见问题

### 解压后提示 Offline startup failed？

确认解压完整，必须存在：

```text
.venv\
.tools\python\
.tools\uv\uv.exe
```

### 提示 `uv trampoline failed to canonicalize script path`？

这是旧包用 `.venv\Scripts\drpa-client.exe`（uv 启动器）导致的，该 exe 内嵌了打包机的绝对路径。
请使用最新离线包，并通过 `scripts\run-drpa-windows.bat` 启动；脚本已改为 `python -m` 方式。

### 开发机需要联网？

```bat
set DRPA_ALLOW_ONLINE=1
scripts\run-drpa-windows.bat
```
