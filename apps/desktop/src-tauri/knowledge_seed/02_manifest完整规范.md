# manifest.yaml 完整规范（schema 2）

`manifest.yaml` 是 RPAZ 的机器可验证合同。Host 在安装、开发态运行和构建时解析它；字段写进 YAML 但当前模型没有定义时，不会自动获得对应能力。

## 1. 完整结构

```yaml
schema: 2
id: com.example.invoice-export
name: 发票导出
version: 1.2.0

entrypoint:
  runtime: python
  module: main.py
  callable: main

runtime:
  python: "3.11.*"
  lock: requirements.lock

capabilities:
  network:
    allow:
      - "invoice.example.com"
      - "api.example.com"
  filesystem:
    read:
      - "$inputs"
    write:
      - "$outputs"

parameters:
  - id: month
    type: string
    required: true
    default: "2026-07"
  - id: max_pages
    type: number
    required: false
    default: 20
  - id: headless
    type: boolean
    required: false
    default: true
  - id: account_password
    type: secret
    required: true
  - id: input_file
    type: file
    required: false
  - id: export_directory
    type: directory
    required: false
```

## 2. 顶层字段

| 字段 | 类型 | 必填 | 当前规则 |
|---|---|---:|---|
| `schema` | integer | 是 | 当前只接受 `2` |
| `id` | string | 是 | 全局包标识，见下一节 |
| `name` | string | 是 | 去除空白后必须非空，可使用中文 |
| `version` | string | 是 | 三段数字，可带 `-预发布后缀` |
| `entrypoint` | object | 是 | Python 或 command 入口 |
| `runtime` | object | 是 | Python 版本和可选锁文件 |
| `capabilities` | object | 否 | 缺失时网络/文件列表均为空 |
| `parameters` | array | 否 | 缺失时默认为空数组 |

## 3. id 规则

合法示例：

```text
com.example.invoice-export
local.1234abcd
cn.company.department-task
```

验证规则：

- 整体最长 128 字符。
- 必须至少包含一个 `.`。
- 每段最长 63 字符，不能为空。
- 每段只允许 ASCII 小写字母、数字和 `-`。
- 每段不能以 `-` 开头或结尾。

以下都不合法：

```text
invoice                 # 没有点
Com.Example.Task        # 大写
com.example.my_task     # 下划线
com..task               # 空段
com.example.-task       # 段以连字符开头
```

`id` 是安装目录和运行记录的稳定身份。项目显示名称变化时不要随意更换 ID；如果业务含义完全不同，应创建新 ID。

## 4. version 规则

当前验证接受三段纯数字核心版本：

```text
0.1.0
1.0.0
2.15.3
1.2.0-preview.1
```

核心必须恰好三段。`1.2`、`v1.2.3`、`1.2.x` 都不通过。预发布后缀当前不做完整 SemVer 字符集验证，但仍建议使用标准 SemVer。

发布新内容时递增版本，避免同一 `id + version` 对应不同代码。

## 5. entrypoint

### Python 入口

```yaml
entrypoint:
  runtime: python
  module: src/main.py
  callable: main
```

- `module` 是包内相对路径。
- 只能使用 `/`，不能使用 `\`。
- 不允许绝对路径、盘符、冒号、空路径、`.` 或 `..` 路径段。
- `callable` 去除空白后必须非空。
- Runtime 动态加载模块后调用 `callable(ctx)`。

### Command 入口

模型也支持：

```yaml
entrypoint:
  runtime: command
  executable: bin/task.exe
  args:
    - "--mode"
    - "offline"
```

`executable` 仍必须是包内安全相对路径，`args` 缺失时为空数组。当前 Windows 主流程重点验证 Python RPAZ；在依赖 command 入口前，应在目标发布构建中进行实际回归。

## 6. runtime

```yaml
runtime:
  python: "3.11.*"
  lock: requirements.lock
```

| 字段 | 类型 | 说明 |
|---|---|---|
| `python` | string / null | 期望 Python 版本表达式 |
| `lock` | string / null | 包内依赖锁文件路径 |

当前桌面版使用随安装包提供的 sealed Python 3.11。脚本不应调用系统 Python，不应在用户离线环境执行在线 `pip install`。`lock` 路径会做安全相对路径校验；它用于声明可重复依赖集合，不代表 Host 会从互联网下载依赖。

标准库优先。需要平台已有第三方库时，先确认离线运行时清单。需要新库时，应在全量运行时构建阶段锁定 wheel 并测试，而不是在任务启动时安装。

## 7. capabilities

缺失 `capabilities` 等价于：

```yaml
capabilities:
  network:
    allow: []
  filesystem:
    read: []
    write: []
```

### network.allow

声明任务预期访问的主机名：

```yaml
network:
  allow:
    - "www.bing.com"
    - "api.example.com"
```

只写主机，不写 `https://`、路径或查询字符串。重定向可能访问第二个主机，应把实际需要的域名都写入清单。能力声明是包合同的一部分，即使当前预览 Host 尚未对每个请求执行网络代理拦截，也应准确维护。

### filesystem

```yaml
filesystem:
  read:
    - "$inputs"
  write:
    - "$outputs"
```

默认任务输出应写 `$outputs`。路径参数应通过 `file` / `directory` 类型进入任务配置，并在代码中验证存在性、类型和大小。不要为了方便声明整个磁盘。

## 8. parameters 参数数组

每个参数当前支持四个字段：

| 字段 | 类型 | 必填 | 说明 |
|---|---|---:|---|
| `id` | string | 是 | Python 从 `ctx.params[id]` 读取 |
| `type` | enum | 是 | `string/number/boolean/secret/file/directory` |
| `required` | boolean | 否 | 默认 `false` |
| `default` | JSON 兼容值 | 否 | 默认不存在 |

### 参数 ID

- 最长 64 字符。
- 只允许 ASCII 小写字母、数字和 `_`。
- 同一清单中不能重复。

推荐：`start_date`、`max_results`、`headless`。避免：`StartDate`、`max-results`、`开始日期`。

### 类型与 Python 读取

```python
name = str(ctx.params.get("name") or "")
count = int(ctx.params.get("count") or 1)
ratio = float(ctx.params.get("ratio") or 0.5)
headless = bool(ctx.params.get("headless", False))
source = Path(str(ctx.params["source_file"]))
```

`secret` 应由凭据系统提交引用或运行期值，日志中不要输出。`file` / `directory` 是路径字符串，代码仍需 `Path(...).expanduser()` 或业务需要的解析，并检查目标。

### 显式默认值

可选参数应尽量有默认值：

```yaml
- id: market
  type: string
  required: false
  default: zh-CN
- id: image_count
  type: number
  required: false
  default: 1
- id: headless
  type: boolean
  required: false
  default: false
```

默认值类型应与参数类型匹配。当前 Rust schema 会保存 JSON 值，但业务代码仍应做范围和格式校验。

## 9. YAML 书写建议

- 使用两个空格缩进，禁用 Tab。
- 版本表达式和可能被 YAML 误判的字符串加引号。
- 日期、前导零编号、`yes/no/on/off` 等值建议加引号。
- 域名列表每行一个值。
- 保持字段顺序：身份 → 入口 → runtime → capabilities → parameters。

## 10. 校验清单

- [ ] schema 等于 2。
- [ ] ID 符合域名式小写规则。
- [ ] version 是三段数字。
- [ ] name 非空。
- [ ] Python module 路径存在且不越界。
- [ ] callable 在模块中定义且可调用。
- [ ] lock 路径存在时使用包内相对路径。
- [ ] 参数 ID 合法且不重复。
- [ ] 可选参数具有清晰默认值。
- [ ] 网络域名和读写目录与代码行为一致。

继续阅读 [ctx 上下文与默认配置](./03_ctx上下文与默认配置.md)，了解这些字段在执行时如何变成 Python 对象。

