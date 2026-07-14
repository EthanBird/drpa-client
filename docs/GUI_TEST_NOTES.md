# GUI 测试记录

日期：2026-07-14
环境：Windows，本地 `npm run tauri:dev`，真实 Tauri/WebView 窗口。

## 已验证流程

- 总览页：空工作区可正常加载，Host 显示已连接。
- 自动化计划页：可从命令面板进入；真实空 Host 数据目录下显示空态。
- 脚本包页：无已安装包时显示空态与安装/新建入口。
- 开发工作室：
  - 可新建中文名称项目。
  - 项目 ID 自动生成，热重启后项目仍能恢复。
  - `main.py`、`manifest.yaml`、`notebook.ipynb` 文件列表正常。
  - 真实 Tauri 可访问性树可见“新建文件”“新建文件夹”“导入文件”三个文件操作入口。
  - Studio 直接运行成功后，运行记录页出现成功记录，进度为 100%。
- Bing 每日一图：使用真实示例目录和 `zh-CN`、`image_count=1` 参数运行，事件序列完整到 `completed(exit_code=0)`，产出 JSON 元数据和 JPG 图片。
- 前端回归：右键文件菜单、创建并选中任务配置、命令面板 Enter 执行均由 Vitest/Testing Library 覆盖。

## 发现并已修复

1. **Studio 直接运行失败，提示 `stream did not contain valid UTF-8`**
   - 现象：开发模式直接运行新建项目时，Python worker 输出中文 JSONL 后 Host 读取失败。
   - 根因：Windows Python 子进程 stdout 使用本地代码页，协议通道要求 UTF-8。
   - 修复：启动 Studio Kernel 和 worker 时设置 `PYTHONUTF8=1`、`PYTHONIOENCODING=utf-8`；debug 模式优先使用仓库 `.venv` Python。

2. **运行后 Studio 文件列表显示 `__pycache__/main.cpython-311.pyc`**
   - 现象：项目运行后 Python 字节码缓存进入 Studio 文件树，存在被导出进 `.rpaz` 的风险。
   - 修复：项目文件收集时过滤 `__pycache__`、`.pyc`、`.pyo`。

3. **命令面板无法搜索进入“运行记录”，且输入框 Enter 不执行首个结果**
   - 现象：搜索“记录”无结果；搜索后需要额外 Tab 才能选择命令。
   - 修复：新增“打开运行记录”命令；输入框按 Enter 执行首个匹配命令。

4. **开发工作室缺少文件管理入口**
   - 修复：文件面板增加右键菜单和顶部快捷按钮，支持新建文件、新建文件夹、文件选择导入、浏览器文件拖拽，以及 Tauri 原生路径拖拽导入。
   - 回归：项目文件列表现在包含目录项，并继续过滤 `__pycache__`、`.pyc`、`.pyo`。

5. **脚本包参数没有显式默认值，运行工作台缺少“创建任务配置”**
   - 修复：manifest 参数模型、Host 摘要协议和前端模型贯通 `default/defaultValue`；Bing 示例显式声明 `market=zh-CN`、`image_count=1`。
   - 修复：运行工作台可以创建本地任务配置、持久化参数、重置到 manifest 默认值，并复用脚本包的运行 profile 提交任务。

## 仍需后续设计确认

- 自动化计划页当前是只读预览。真实 Host 数据只有带 schedule 的 profile 才会派生计划；现有示例 `.rpaz` 没有 schedule，因此该页在真实桌面 Host 中为空。
- 当前阶段“新建计划”按钮保持 disabled，后续 P2 Scheduler 需要补创建向导、持久化、触发器计算和执行语义。
- Computer-use 截图接口在该 Tauri 窗口上返回 `SetIsBorderRequired failed`，本次使用可访问性树检查真实窗口，并用前端集成测试补齐右键菜单交互覆盖；这属于测试工具与窗口捕获兼容性记录，不影响应用运行。
