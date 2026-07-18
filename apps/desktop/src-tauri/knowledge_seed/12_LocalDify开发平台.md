# Local Dify 开发平台

DRPA 的“AI 应用”模块用于在本机创建、测试和发布轻量 Dify 兼容应用。第一阶段支持 Chat 与 Completion；Workflow 画布和图执行器将在后续阶段加入。

## 开发流程

1. 打开“基础设施 → AI 应用”。
2. 在 Providers 中配置 OpenAI 兼容 URL、Model 和 API Key。
3. 新建 Chat 或 Completion 应用。
4. 编辑系统指令、开场白、输入变量、Temperature 和最大输出。
5. 在“调试预览”中运行，观察流式 Markdown、Token 和耗时。
6. 在“运行记录”中查看完整输入、输出和错误。
7. 发布本地版本，获取应用 API Token。
8. 导出 Dify YAML DSL，上传到 Dify Cloud 或自部署 Dify。

## Provider 与 Dify Loves Hermes

Provider 接受标准 OpenAI Chat Completions 接口：

```text
https://api.example.com/v1
http://127.0.0.1:34121/v1
```

第二个地址可以指向 `dify-loves-hermes`。这样 Local Dify 会把请求作为 OpenAI 请求交给 Hermes，再由 Hermes 调用本地或远程 Dify App API。

DRPA 会通过以下 Header 记录调用链：

```text
X-DRPA-Trace-Id
X-DRPA-Provider-Route
X-DRPA-Hop-Count
```

路由重复出现同一 App 或超过四跳时，运行会以明确错误结束，避免 Provider 套娃形成循环。

## 本地 Dify Service API

在“API 与导出”中启动服务，默认地址：

```text
http://127.0.0.1:34130/v1
```

发布应用后获取 Token：

```bash
curl http://127.0.0.1:34130/v1/chat-messages \
  -H "Authorization: Bearer APP_TOKEN" \
  -H "Content-Type: application/json" \
  -d '{
    "inputs": {},
    "query": "测试本地应用",
    "response_mode": "streaming",
    "user": "developer"
  }'
```

支持：

- `GET /v1/health`
- `GET /v1/parameters`
- `POST /v1/chat-messages`
- `POST /v1/completion-messages`
- `POST /v1/workflows/run`

## DSL 兼容性

导入 DSL 时，DRPA 保留原始 YAML，再生成本地规范化应用。导出前会检查：

- 应用是否选择 Provider。
- 模式是否已经进入本地执行器。
- 是否配置 Dify 云端 Provider 和 Model 映射。
- Provider 是否仍指向 `localhost`。

本机 URL 和缺少云端映射会产生 warning；缺少 Provider 或尚未支持的执行模式会产生 error。

## 文件与运行记录

```text
workspace/local-dify/
├── apps/<app-id>/app.json
├── apps/<app-id>/source.dify.yml
├── providers.json
├── secrets.json
└── runtime.sqlite3
```

应用与 Provider 配置可备份和审查；API Key 与应用 Token不会写入导出的 Dify DSL。运行历史保存在 SQLite，可在应用内查看。
