# DRPA Next Windows 全量离线版

Windows x64 发行只提供完整 Setup，包含 DRPA 桌面端、CPython 3.11、完整离线依赖、真实 Jupyter Kernel、Chrome for Testing、Fixed Version WebView2、JCode 开发者 Agent sidecar 和 Bing 每日一图示例包。目标机器无需预装 Python、Node.js、浏览器或联网下载 pip 依赖。

## 安装与升级

1. 下载 `drpa-next-<version>-windows-x86_64-setup.exe`。
2. 退出正在运行的 DRPA。
3. 首次安装时选择非系统盘目录；升级时选择原安装目录。
4. 启动后打开“运行环境”执行验证，再运行一个 RPAZ 检查日志和产物。

安装器不申请管理员权限、不注册卸载项、不读写应用注册表。升级会替换应用和随包运行时，但跳过安装目录中的 `data/`，因此 RPAZ 包、项目、会话、运行历史、知识文档、配置和产物会保留。

```text
<install>/
├── DRPA Next.exe
├── install-manifest.json
├── runtime/
├── webview2/
├── jcode/
├── examples/
└── data/                 包、项目、环境、历史、产物和配置
```

不要只复制 `DRPA Next.exe`、单独移动 `runtime/`，也不要把已初始化的 `data/runtime-environment` 复制到不同安装路径。生成环境含最终路径信息；移动后应在“运行环境”中确认并执行强制重建。

## 全量发布策略

- Windows Release 固定包含 Setup、`install-manifest.json` 和 Bing 示例 `.rpaz` 三个资产。
- 不发布 `.drpa-update`，设置页也不提供本地增量更新入口。
- 普通开发提交不触发 Windows 发行；手工运行 workflow 或使用 `release(windows):` 提交前缀才构建全量包。
- 每次发行都重新构建 sealed runtime、JCode、Fixed Version WebView2 和 NSIS Setup，并执行断网 bootstrap 与 Python/Jupyter/Chrome 导入检查。
- `install-manifest.json` 继续作为安装内容审计清单使用，不作为增量更新协议。

完整 stage 中的文件必须是实体文件。库存生成器拒绝 symlink、Junction 和 reparse point，防止安装包在构建机器上通过、复制到离线机器后才暴露缺失依赖。

## 内存策略

Windows WebView2 默认关闭 GPU 合成、后台联网、组件更新、扩展和同步。DRPA 的界面是本地二维工作台，不依赖 WebGL；该策略减少 GPU 进程的大块私有提交，同时保留普通页面请求、文件下载和本地服务访问。

除首页外的业务页面按导航懒加载。Monaco、Markdown、流程设计器、插件管理和 Agent 对话只在进入对应工作台后载入，退出页面时由 React 卸载页面状态和监听器。

## 卸载与备份

由于安装器不写注册表，系统“应用和功能”中不会出现卸载项。退出 DRPA 后，备份需要保留的 `data/`，再删除整个安装目录和快捷方式即可。

升级或迁移前可备份整个 `data/`。恢复到不同目录后先运行环境验证；如果 Python 环境损坏，使用带确认窗口的“强制重建生成环境”，不要删除 `packages/`、`projects/` 或 `runs/`。

开发和发布流程见 [`DEVELOPMENT.md`](DEVELOPMENT.md)，完整依赖处理见 [`../offline/README.md`](../offline/README.md)。
