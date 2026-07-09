# DRPA Client 使用文档

本文档面向最终用户，说明如何安装依赖、安装脚本包、编辑参数、保存任务配置、运行任务和查看结果。

## 1. 启动程序

在项目目录运行：

```bash
uv sync
uv run drpa-client
```

如果不用 uv，也可以使用：

```bash
python -m pip install -e .
drpa-client
```

## 2. 目录说明

DRPA Client 使用项目目录下的固定目录：

```text
.venv/          Python 运行环境，所有脚本包共享
.drpa-data/     数据目录
wheelhouse/     离线依赖 wheels
examples/       示例脚本包
```

用户通常不需要手动管理 `.venv`。

## 3. 工作台概念

DRPA Client 的工作台参考 Docker 的思路：

| 概念 | 类比 Docker | 说明 |
| --- | --- | --- |
| 脚本包 | 镜像 image | `.rpaz`、`.zip` 或单个 `.py` 导入后的自动化能力 |
| 任务配置 | 容器配置 | 某个脚本包的一组参数，可以保存并下次复用 |
| 运行实例 | 容器 container | 一次实际运行，有独立 run id 和日志 |

工作台分为三栏：

```text
左侧：脚本包列表
中间：任务配置列表
右侧：参数表、运行表单、运行实例、日志
```

## 4. 安装脚本包

### 4.1 点击安装

进入“工作台”，点击：

```text
安装脚本包 / 导入 py
```

支持选择：

- `.rpaz`
- `.zip`
- `.py`

默认打开项目目录，方便选择：

```text
examples/*.rpaz
```

### 4.2 拖拽安装

也可以在任意页面把文件拖进窗口：

- `.rpaz`
- `.zip`

程序会自动切换到工作台并安装。

### 4.3 依赖安装

安装时勾选：

```text
安装依赖到项目 .venv
```

依赖安装规则：

- 禁止联网。
- 只使用本地 wheelhouse 和脚本包内 wheels。
- 全局 wheelhouse 优先。
- `.rpaz` 内 wheels 其次。

## 5. 参数表和运行表单

选中一个脚本包后，右侧会显示：

```text
参数表
运行参数
```

### 5.1 参数表

参数表来自脚本包的 `manifest.yaml -> params`。

表格字段：

- 参数名
- 类型
- 必填
- 默认值
- 当前值
- 说明

其中：

```text
当前值
```

可以直接编辑。

修改“当前值”后，下方运行表单会同步更新。

### 5.2 运行表单

运行表单也可以修改参数。

表单修改后，参数表里的“当前值”会同步更新。

支持的参数类型：

- string
- password
- boolean
- integer
- number
- date
- file
- directory

## 6. 保存任务配置

填写好参数后，可以点击：

```text
新建任务
```

输入任务名称后，会保存为一个任务配置。

以后可以在中间的“任务配置”列表中选择它。

如果修改了参数，可以点击：

```text
保存参数
```

这样下次选择这个任务配置时，会自动回填上次保存的参数。

删除任务配置：

```text
删除任务
```

## 7. 运行任务

### 7.1 临时运行

可以直接修改右侧参数，然后点击：

```text
运行
```

这会创建一个临时运行实例。

### 7.2 运行保存的任务配置

在中间选择任务配置，然后点击：

```text
运行选中任务
```

### 7.3 并发运行

可以启动多个任务。

每次运行都会出现在：

```text
运行实例
```

每个运行实例都有独立日志。

点击不同运行实例，可以切换查看对应日志。

### 7.4 停止任务

选择一个运行实例，然后点击：

```text
停止
```

只会停止当前选中的运行实例。

## 8. 查看运行历史

进入：

```text
运行历史
```

可以查看：

- 开始时间
- 脚本包
- 版本
- 状态
- 退出码
- 输出目录
- 日志文件

## 9. 示例脚本包

仓库提供以下示例：

```text
examples/hello_web_bot.rpaz
examples/bing_daily_image.rpaz
examples/bilibili_search.rpaz
examples/marketing_cancel_order_dispatch.rpaz
```

说明：

- `hello_web_bot`：最小任务示例。
- `bing_daily_image`：下载 Bing 每日一图。
- `bilibili_search`：使用 DrissionPage 搜索 Bilibili。
- `marketing_cancel_order_dispatch`：营销销户派工自动化示例。

## 10. 代码编辑

进入：

```text
代码编辑
```

可以打开并编辑：

- `.py`
- `.yaml`
- `.yml`
- `.json`
- `.md`
- `.txt`

编辑器使用 Monaco Editor。

如果提示：

```text
Monaco Editor 未加载
```

通常是当前环境无法访问 Monaco CDN。后续产品化可以把 Monaco 静态资源内置到安装包。

## 11. 浏览器录制

浏览器录制是高级功能，默认隐藏。

开启方式：

1. 打开“设置”。
2. 勾选：
   ```text
   启用高级功能：浏览器录制
   ```
3. 保存设置。

录制器生成的是草稿脚本包，需要人工检查和修改。

## 12. 常见问题

### 12.1 为什么不能联网安装依赖？

为了保证内网和离线环境可控，DRPA Client 强制使用本地 wheels：

```bash
--no-index
```

### 12.2 为什么只有一个 `.venv`？

这是为了让用户无感使用，不需要理解 Python 环境。

所有依赖统一安装到：

```text
.venv/
```

### 12.3 参数保存在哪里？

任务配置保存在：

```text
.drpa-data/task-profiles.json
```

### 12.4 输出文件在哪里？

运行输出保存在：

```text
.drpa-data/outputs/
```

### 12.5 日志在哪里？

运行日志保存在：

```text
.drpa-data/logs/
```
