# Dify2API 网关

这个内置能力包把 Dify Agent 暴露为本机 OpenAI 兼容服务。它由 DRPA
负责启动、停止、健康检查和日志收集，不会再打开独立 GUI。

## 能力

- `GET /v1/models`
- `POST /v1/chat/completions`，支持流式与非流式响应
- OpenAI `tools` / `tool_calls` 适配
- Dify 上游连通性测试
- 普通对话与示例工具调用调试
- OpenAI 请求、Dify 请求、SSE 事件与最终响应的结构化链路调试

## 安全边界

- 服务默认只监听 `127.0.0.1`。
- Dify App API Key 只用于访问上游。
- Dify 上游 HTTPS 客户端允许自签名或内部 CA 证书，不校验证书链与主机名；仅应连接可信内网服务。
- 本地客户端使用独立生成的 `proxy_api_key`。
- 密钥只保存在 Host 管理的工作区状态文件中；Windows 使用当前账号 ACL，Unix 使用 `0600`。
- 插件摘要只返回“是否已配置”，不会把密钥发送到 WebView。
- 调试流量只进入 DRPA 的有界内存日志，不写入插件包。
- gateway 意外退出时由 DRPA 宿主自动拉起；用户显式停止或禁用插件时不会重启。
- 收到正常终止信号时会先把信号原因写入服务日志，便于区分宿主停止与系统异常回收。
- 请勿把 `state.json`、运行日志或任何真实 API Key 打包分发。

修改配置后需要重启插件服务。启动成功后，可从 Provider 卡片把
Endpoint、模型名和不透明凭据引用应用到 AI Agent；模型请求由 Rust
Host 在当前工作区解析并注入本地代理 Key。
