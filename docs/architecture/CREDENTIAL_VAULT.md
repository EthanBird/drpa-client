# 开发凭据保险箱安全架构

## 1. 目标与边界

DRPA 凭据保险箱用于本机保存开发账号、API Key、访问令牌、数据库凭据、SSH 信息和安全备注，并向桌面 UI、RPAZ Agent、JCode Agent 与显式启动的 loopback API 提供统一读写能力。

设计目标：

- 默认离线，不依赖云端账号、短信或网络校验；
- 静态数据中不出现凭据名称、账号、URL、标签、备注和 secret 明文；
- 初始化与解锁使用 Google Authenticator 兼容的 6 位 TOTP；
- 运行时授权在退出、手动锁定或 24 小时到期时失效；
- 恢复码可以独立解锁，成功使用后立即轮换；
- 外部接口仅显式启动、仅监听 loopback，并在 TOTP 验证后签发随机 Bearer Token；
- AI 工具复用同一 Host 权限边界，不直接读取 `vault.json`。

本方案不把 6 位 TOTP 当作高熵加密密码。TOTP 用于本地用户在场验证；真正的静态加密依赖随机 256-bit Vault Key 和操作系统设备保护。Windows 当前用户上下文已经完全失守时，DPAPI 保护的数据也处于同一信任边界。

## 2. 数据与密钥层级

```text
随机 Vault Key (256 bit)
  ├─ AES-256-GCM → 完整 Vault Payload
  │                 └─ name/kind/username/secret/uri/notes/tags/timestamps
  ├─ AES-256-GCM key-wrap ← HKDF(deviceSecret || totpSeed, vaultId)
  │                          ├─ deviceSecret → Windows DPAPI
  │                          └─ totpSeed     → Windows DPAPI
  └─ AES-256-GCM key-wrap ← HKDF(recoveryCode, recoverySalt)
                             └─ recoveryCode 只在初始化/轮换时展示
```

实现位于 `apps/desktop/src-tauri/src/credential_vault.rs`。密文文件为安装数据根目录下的 `credential-vault/vault.json`，不跟随单个工作区切换。Payload 每次写入递增 revision，并把 `vaultId + revision + 用途标签` 作为 AES-GCM AAD，避免不同字段或旧修订密文被替换复用。写入先落临时文件，再以备份和 rename 提交。

Windows 使用 DPAPI `CryptProtectData`/`CryptUnprotectData` 保护 TOTP seed 与随机 device secret；密文与当前 Windows 用户凭据绑定。非 Windows 兼容实现使用本地 256-bit device key 和 `0600` 权限，后续应替换为 Secret Service、KWallet 或 Keychain adapter。

采用标准依据：

- TOTP 算法、30 秒时间步与动态截断遵循 [RFC 6238](https://www.rfc-editor.org/rfc/rfc6238)。
- 二维码使用 Google Authenticator 的 [`otpauth://` Key URI 格式](https://github.com/google/google-authenticator/wiki/Key-Uri-Format)，参数固定为 SHA1、6 位、30 秒。
- Windows 本机保护使用微软 [CryptProtectData / DPAPI](https://learn.microsoft.com/windows/win32/api/dpapi/nf-dpapi-cryptprotectdata)。
- 完整 Payload 使用 AES-256-GCM；产品分层参考 Bitwarden 对[加密和密钥派生](https://bitwarden.com/help/what-encryption-is-used/)的公开说明，但 DRPA 使用自己的本地设备密钥与 TOTP gate，不复用 Bitwarden 协议或服务。

## 3. 初始化、解锁和恢复

### 初始化

1. Host 生成 160-bit TOTP seed，并只在内存保存 10 分钟。
2. UI 渲染 `otpauth://` 二维码和 Base32 手动密钥。
3. 用户在 Google Authenticator 扫码，输入 6 位验证码。
4. Host 允许当前、前一和后一时间步，验证成功后生成 Vault Key、device secret 和恢复码。
5. Host 写入加密 envelope，安装 24 小时内存会话，并且只在本步骤返回恢复码。

### 日常解锁

1. UI 或 `/session/verify` 提交 6 位 TOTP。
2. Host 先执行进程内渐进式失败退避，再读取并 DPAPI 解封 TOTP seed/device secret。
3. TOTP 通过后派生 wrapping key、解封 Vault Key，并安装会话。
4. 会话包含 `Zeroizing<[u8; 32]>` Vault Key、256-bit 随机服务 Token 和绝对过期时间。

退出进程会释放全部会话状态；手动锁定和 24 小时到期会立即丢弃 Vault Key 与服务 Token。`zeroize` 降低正常释放后的内存残留，不宣称抵御具有同进程读内存能力的调试器或内核攻击者。

### 恢复码

恢复码来自 160-bit 随机数，经 Base32 分组展示。它通过独立 salt + HKDF 派生 wrapping key，因此不依赖 DPAPI 或 TOTP seed。恢复解锁成功后 Host 立即生成新的恢复码和 salt、重写 key-wrap，并只返回新恢复码一次。恢复码文件应离线保存，不与 `vault.json` 放在同一位置。

## 4. UI 与数据模型

桌面页采用三栏凭据管理布局：

- 左栏：全部、收藏、凭据类型和本地服务状态；
- 中栏：名称、账号、地址、标签搜索和条目列表；
- 右栏：创建、编辑、显隐/复制 secret、收藏、标签、备注和删除确认；
- 初始化：安全说明、二维码、手动密钥和 6 位验证码；
- 锁定：TOTP 与恢复码两个入口；
- 恢复：一次性展示、复制和下载恢复码。

支持类型：`login`、`apiKey`、`token`、`database`、`ssh`、`secureNote`。列表 API 返回 summary，不包含 `secret` 和 `notes`；详情 API 只在解锁状态下返回完整条目。单个保险箱最多 2,000 项，所有字符串与数组仍由 Rust Host 做长度和类型校验。

## 5. Loopback 服务协议

服务默认停止。用户在已解锁页面中手工启动后只绑定 `127.0.0.1`，默认端口 `34131`。服务不返回 permissive CORS header，所有响应使用 `Cache-Control: no-store` 和 `X-Content-Type-Options: nosniff`，请求体最大 256 KiB。

```http
GET /v1/vault/health

POST /v1/vault/session/verify
Content-Type: application/json

{"code":"123456"}
```

验证成功响应：

```json
{"token":"RUNTIME_BEARER_TOKEN","expiresAt":1785900000}
```

后续请求：

```http
Authorization: Bearer RUNTIME_BEARER_TOKEN
```

| Method | Path | 用途 |
| --- | --- | --- |
| `GET` | `/v1/vault/credentials` | 列出不含 secret 的摘要 |
| `POST` | `/v1/vault/credentials` | 新建条目 |
| `GET` | `/v1/vault/credentials/{id}` | 读取完整条目 |
| `PUT` | `/v1/vault/credentials/{id}` | 更新条目 |
| `DELETE` | `/v1/vault/credentials/{id}` | 删除条目 |

服务 Token 与 UI 解锁使用同一会话；手动锁定、到期或退出会使它失效。不要把 Token 写入日志、命令历史、项目文件或长期配置。当前 API 面向同一台机器上的开发服务，不监听 LAN，也没有跨用户 IPC 身份认证。

## 6. AI Agent 接入

RPAZ Agent 和 JCode MCP 都暴露三个工具：

| 工具 | 行为 |
| --- | --- |
| `vault_list_credentials` | 列出摘要，不返回 secret |
| `vault_get_credential` | 按 ID 向当前模型工具回合提供完整凭据 |
| `vault_upsert_credential` | 创建或更新本地加密条目 |

工具执行前 Host 必须发现有效的同进程 Vault Session。`vault_get_credential` 的原始结果只进入当前模型的 `role=tool` 消息；持久会话和 UI 工具时间线保存的是 redacted 事件，不保存 secret。模型 Provider 仍会收到该次工具返回，因此应只对可信 Provider 开启并请求所需的最小条目。列表与写入事件同样不应回显 secret。

JCode 通过 `DRPA_AGENT_BRIDGE_ENDPOINT` + 随机 bridge token 调用 Rust Host，不直接解析保险箱文件。bridge token 只注入当前 sidecar/MCP 进程环境。

## 7. 威胁模型与维护规则

已覆盖：

- 复制 `vault.json` 后离线查看明文；
- 篡改 Payload、nonce、key-wrap 或 revision；
- 未解锁的普通 UI/Tauri/Agent 读取；
- 网络其他主机直接访问本地服务；
- 长期复用一次验证码或服务 Token；
- 恢复码二次使用。

不覆盖：

- 已取得当前 Windows 用户会话并能调用 DPAPI 的恶意进程；
- 管理员/内核、进程注入、内存抓取或键盘录制；
- 用户主动把 secret 提交给不可信模型 Provider；
- 被替换的 DRPA 可执行文件或未签名安装包供应链攻击；
- 恢复码和 `vault.json` 同时泄露。

维护约束：

1. WebView 不直接读写 `vault.json`，全部经过 typed command。
2. summary DTO 不增加 secret/notes；详情 DTO 不写日志或运行历史。
3. 新的消费者优先保存 credential ID，在使用时由 Host 解析。
4. 扩展 loopback API 时继续限制方法、body、header 和响应缓存，不开放通用命令代理。
5. 变更 envelope、KDF 或 AAD 时递增 schema 并写迁移；旧数据不得静默重置。
6. 发布测试至少覆盖 RFC TOTP 向量、加密往返、密文无明文、锁定、恢复轮换、Tauri DTO 和前端初始化流程。
