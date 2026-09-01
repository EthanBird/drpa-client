# DRPA Core stdio 协议

`drpa serve --stdio` 启动不依赖 WebView2 的本地 Core 服务。协议使用 UTF-8 JSON Lines：一个请求、一条响应；长时间运行 RPAZ 时可在最终响应前发送事件。

## 请求与响应

```json
{"protocol":1,"id":"request-1","method":"ping","params":{}}
```

```json
{"protocol":1,"type":"response","id":"request-1","ok":true,"result":{"protocol":1,"version":"3.0.0","platform":"windows-x86_64"}}
```

错误响应包含稳定的 `error.code` 和可读的 `error.message`。单条请求最大 4 MiB。stdio 服务顺序执行请求，避免多个组件事务或工作区切换互相覆盖。

## 方法

- `ping`
- `status`
- `shutdown`
- `component.list`
- `component.install`：`{"path":"...drpac"}`
- `component.remove`：`{"id":"...","purge":true}`
- `workspace.list`
- `workspace.create`：`{"name":"..."}`
- `workspace.use`：`{"id":"workspace-..."}`
- `runtime.list`
- `runtime.select`：`{"profileId":"org.drpa.python-runtime.py314-minimal"}`
- `runtime.status`
- `rpaz.list`
- `rpaz.install`：`{"path":"...rpaz"}`
- `rpaz.uninstall`：`{"id":"..."}`
- `rpaz.run`：`{"packageId":"...","profileId":"...","parameters":{}}`

`rpaz.run` 会发送事件：

```json
{"protocol":1,"type":"event","requestId":"request-2","event":"rpaz.run.event","payload":{"type":"progress","sequence":3,"value":50,"message":"处理中"}}
```

最终响应的 `result` 是持久化后的完整运行详情。

## 进程边界

该协议只通过父子进程的 stdin/stdout 暴露，不监听 TCP 端口，不需要管理员权限。未来桌面 UI、headless 管理端和独立 Browser Host 应复用此协议，而不是重复实现组件、工作区和 RPAZ 状态逻辑。
