# Studio Notebook 与 VS Code Jupyter 对照

DRPA Next 没有把 `microsoft/vscode-jupyter` 扩展包直接塞进 Tauri。该扩展依赖 VS Code Extension Host、内置 Notebook UI 和 `NotebookController` API，原样复制既不能启动，也会把完整 VS Code 平台变成新的离线依赖。当前实现固定参考了上游提交 `b3184399d184e847037a6823fdc4976ec99ca530`，移植其可复用的生命周期、执行与 notebook 数据模型。

## 源码映射

| VS Code Jupyter 源码职责 | DRPA Next 实现 |
| --- | --- |
| `src/kernels/kernelProvider.base.ts`：以 notebook/id 管理 Kernel 生命周期和状态 | Rust `StudioKernelManager` 以项目 ID 管理一个持久的 sealed-Python 子进程 |
| `src/kernels/kernelExecution.ts`：执行队列、执行计数和输出 | JSONL 单请求串行协议、递增 `execution_count`、stdout/stderr/result/error |
| Jupyter `complete_request`：基于代码和实时命名空间补全 | Monaco completion provider 将 UTF-16 光标转换为 Unicode code point，Rust 转发 JSONL `complete`，bridge 调用标准 Jupyter completion |
| `src/notebooks/controllers/vscodeNotebookController.ts`：连接 VS Code Notebook UI | React + Monaco 的本地 Notebook 工作区，读写标准 nbformat v4 `.ipynb` |
| `src/kernels/raw/session/rawJupyterSession.node.ts`：基于 ZMQ 的原生 Jupyter 会话 | 内置 `ipykernel`、`jupyter_client` 与 `pyzmq`，由 Python bridge 管理真实 Jupyter shell/iopub/control 通道；Rust Host 只承载进程生命周期与 JSONL IPC |

参考源码：

- <https://github.com/microsoft/vscode-jupyter/blob/b3184399d184e847037a6823fdc4976ec99ca530/src/kernels/kernelProvider.base.ts>
- <https://github.com/microsoft/vscode-jupyter/blob/b3184399d184e847037a6823fdc4976ec99ca530/src/kernels/kernelExecution.ts>
- <https://github.com/microsoft/vscode-jupyter/blob/b3184399d184e847037a6823fdc4976ec99ca530/src/notebooks/controllers/vscodeNotebookController.ts>
- <https://github.com/microsoft/vscode-jupyter/blob/b3184399d184e847037a6823fdc4976ec99ca530/src/kernels/raw/session/rawJupyterSession.node.ts>

## 当前能力

- 新项目自动生成可互操作的 nbformat v4 `notebook.ipynb`。
- 代码与 Markdown 单元格增删、Monaco 编辑、单格运行、全部运行。
- 项目级持久命名空间、执行计数、最后表达式结果、stdout/stderr、错误回溯。
- Kernel 重启与变量浏览。
- Python 文件与代码单元离线补全；同时使用当前单元源码和项目级持久 IPython 命名空间，不启动额外语言服务器。
- 真实 IPython Kernel、Jupyter 消息协议与 ZMQ；支持标准 stream、execute_result、display_data、error 输出结构。
- 执行结果回写 `.ipynb`，可在标准 Jupyter/VS Code 中继续打开。
- 打开 Notebook 时异步预热 Kernel；首次运行的运行时定位、进程启动和输出等待都在 Rust blocking worker 中完成，WebView 主线程保持可交互。
- Notebook toolbar 与单元格列表位于受约束的内部滚动区；大量单元格由 `.notebook-scroll` 承载，超长代码单元在 420 px 后启用 Monaco 内部滚动。
- 所有代码都由随安装包封装的 sealed Python 运行，不访问系统 Python 或网络。

## 明确边界

这不是完整 VS Code Extension Host：VS Code 专属命令、扩展市场、`NotebookController` UI 和调试器无法在 Tauri 中原样运行。当前已使用真实 Jupyter wire protocol，但尚不宣称兼容 ipywidgets、远程 Kernel、VS Code Debug Adapter 或全部第三方 MIME Renderer。后续能力继续通过 Jupyter 标准协议扩展，不复制 VS Code 专属平台层。
