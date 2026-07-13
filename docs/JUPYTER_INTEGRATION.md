# Studio Notebook 与 VS Code Jupyter 对照

DRPA Next 没有把 `microsoft/vscode-jupyter` 扩展包直接塞进 Tauri。该扩展依赖 VS Code Extension Host、内置 Notebook UI 和 `NotebookController` API，原样复制既不能启动，也会把完整 VS Code 平台变成新的离线依赖。当前实现固定参考了上游提交 `b3184399d184e847037a6823fdc4976ec99ca530`，移植其可复用的生命周期、执行与 notebook 数据模型。

## 源码映射

| VS Code Jupyter 源码职责 | DRPA Next 实现 |
| --- | --- |
| `src/kernels/kernelProvider.base.ts`：以 notebook/id 管理 Kernel 生命周期和状态 | Rust `StudioKernelManager` 以项目 ID 管理一个持久的 sealed-Python 子进程 |
| `src/kernels/kernelExecution.ts`：执行队列、执行计数和输出 | JSONL 单请求串行协议、递增 `execution_count`、stdout/stderr/result/error |
| `src/notebooks/controllers/vscodeNotebookController.ts`：连接 VS Code Notebook UI | React + Monaco 的本地 Notebook 工作区，读写标准 nbformat v4 `.ipynb` |
| `src/kernels/raw/session/rawJupyterSession.node.ts`：基于 ZMQ 的原生 Jupyter 会话 | 适合离线单机包开发的轻量进程协议，不引入 ZMQ 和 Jupyter Server |

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
- 执行结果回写 `.ipynb`，可在标准 Jupyter/VS Code 中继续打开。
- 所有代码都由随安装包封装的 sealed Python 运行，不访问系统 Python 或网络。

## 明确边界

这不是完整 VS Code Extension Host，也不是完整 Jupyter wire protocol。目前不宣称支持 ipywidgets、富 MIME 渲染、远程 Kernel、ZMQ、调试器或中断长时间运行的单元格。后续增加这些能力时应继续复用 nbformat/Jupyter 协议，不复制 VS Code 专属 UI 层。
