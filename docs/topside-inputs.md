# Topside 设计输入

本页列出设计 `topside` 时必须使用的已观察事实。它不是 `topside` 的设计。本页未出现的行为是未指定。

## 来源

| 来源 | 取用 |
| --- | --- |
| 本仓库 `README.md`、`README.zh-CN.md` | 产品合同 |
| 本仓库 `crates/snorkel` | 1943 拨号端 |
| 本仓库 `.cursor/skills/verify-aqualung/` | 可观察行为与 `phone.py` 握手 |
| [xai-org/grok-build](https://github.com/xai-org/grok-build) 提交 `77cd7eb675ba911c225c3aaeeece3a20cbccc426` | leader 线协议 |
| [ACP v1](https://agentclientprotocol.com/protocol/v1/overview.md) | 手机侧 JSON-RPC |
| crates.io `agent-client-protocol` 2.0.0、`agent-client-protocol-schema` 1.7.0、`agent-client-protocol-http` 2.0.0（2026-08-26） | 官方 Rust SDK 版本 |
| rustls 0.23 `ClientCertVerifier` | 1943 服务端如何验客户端证书 |
| grok-build `crates/codegen/xai-grok-http/src/lib.rs` | `MIN_CLIENT_CONNECT_TIMEOUT` = 120 秒 |
| grok-build `crates/codegen/xai-grok-status-line/src/lib.rs` | `_meta` 键 `clientStatusLine` |

## aqualung 已写明的合同

`aqualung` 是两个程序。`snorkel` 在家里拨出。`topside` 在自己控制的服务器上对手机讲 ACP。代理、工具、对话留在家里。

`snorkel` 存在。`topside` 不存在。接口不稳定。

`snorkel` 用双向 TLS 拨 1943/tcp。服务器只信任一张客户端证书。手机连 7678/tcp，用 bearer token 认证，在 WebSocket 上跑 ACP，每条消息一个 JSON-RPC 对象。

`topside` 对代理是一个远程客户端。对手机是 ACP 服务器。它把多部手机复用到这一条家里连接上。它改写 JSON-RPC 请求 id。它把会话更新扇出给所有正在看这场的手机。它自己应答 `initialize` 和认证。它同一时间只服务一个 `snorkel`。新认证的连接顶掉旧的。

`topside` 状态只在内存里，不写磁盘。它重启之后，手机会重新加载会话再连上。会话本身只存在于家里。

家里的编辑器直连本地 unix socket，不经过服务器。`topside` 挂了，家里的工作不受影响。家里离线时，手机会被告知主机不在。

`topside` 不跑工具、不读文件、也不开终端，更不会替手机注册这些能力。它不存对话、不存工具输出、也不存会话记录。它从不回拨家里。

验证图里的用户路径：

| 功能 | 可观察结果 |
| --- | --- |
| [Phone attach](../.cursor/skills/verify-aqualung/features/phone-attach.md) | 错误 token 在 ACP 开始前失败。正确 token 后 `initialize` 得到 topside 自己的 JSON-RPC 结果或错误，不依赖家里 agent 活着。 |
| [Host away](../.cursor/skills/verify-aqualung/features/host-away.md) | 家里掉线后，已连接的手机收到主机不在。7678 上的 WebSocket 保持。关掉 7678 不是 host-away。 |
| [Snorkel replace](../.cursor/skills/verify-aqualung/features/snorkel-replace.md) | 第二份已鉴权 snorkel 接管。第一份不再是活的家里连接。已连接的手机不被锁在外面。第二张客户端证书是另一个用户，不是替换。 |
| [Session fan-out](../.cursor/skills/verify-aqualung/features/session-fanout.md) | 两部手机看同一场时，一场的更新送到两部。请求 id 碰撞时一部看不到另一部的结果。没在看的手机收不到那场更新。 |
| [Home bypass](../.cursor/skills/verify-aqualung/features/home-bypass.md) | 家里编辑器走 unix socket。杀掉 topside 后家里 socket 仍应答。手机能力里没有 topside 替家里代报的 fs、terminal、tools。 |

本机没有 `target/debug/snorkel` 时，`control-aqualung doctor` 退出 2，`"stage": "design"`。这是本机跑过的。`emit_doctor` 只找到一侧二进制时把 stage 设为 `incomplete` 并退出 1。本机 rustc 1.83 编不了 edition 2024 的 snorkel，所以 `incomplete` 没有用真实二进制跑过。Launch 在缺少 `topside --help` 时拒绝启动。

## snorkel 拨号合同

二进制由 `cargo build -p snorkel` 放进 `target/debug/snorkel`。`--help` 点名 `--socket`、`--server`、`--cert`、`--key`、`--ca`、`--once`。默认端口 1943。五个值也从 `SNORKEL_SOCKET`、`SNORKEL_SERVER`、`SNORKEL_CERT`、`SNORKEL_KEY`、`SNORKEL_CA` 读。坏配置退出 2。信号或 `--once` 完成退出 0。

`Session::establish` 先做 TLS 拨号，再 `UnixStream::connect` 本地 socket。socket 文件不存在时不拨 TLS。一对里任何一侧 EOF 就拆掉两边。重连建立新的 unix 连接和新的 TLS 连接。

TLS 连接超时 10 秒。TCP keepalive 时间 30 秒。失败退避从 500ms 起，上限 30 秒，带 0.8 到 1.0 的抖动。一次会话有字节移动，或持续至少 10 秒，退避计数清零。

`snorkel` 不解析字节，不发 `Register`，不讲 ACP。

## 1943 上的 TLS

`snorkel` 用 rustls 0.23。它带客户端证书链和私钥，并用 `--ca` 里的根证书验服务器。它不设 ALPN。

测试里的假服务器用 `WebPkiClientVerifier::builder(roots).build()`。这只要求客户端证书由该 CA 签发。它不把身份限制成一张叶子证书。

README 写的是服务器只信任一张客户端证书。rustls 0.23 的 `WebPkiClientVerifier` 没有按叶子证书或 SPKI 钉死的开关。`allow_unauthenticated()` 会允许没有客户端证书的连接，方向相反。钉死一张证书要自己实现 `rustls::server::danger::ClientCertVerifier::verify_client_cert`。`CertificateDer` 实现了 `PartialEq`，可以比较整张叶子 DER。也可以先走 `WebPkiClientVerifier` 再比叶子。rustls 维护者在 [PR 1430](https://github.com/rustls/rustls/pull/1430) 里写的是包装现有 verifier，而不是再给一个内置钉死 API。见 [ClientCertVerifier](https://docs.rs/rustls/0.23.37/rustls/server/danger/trait.ClientCertVerifier.html)。

1943 上 TCP 方向是 snorkel 拨入、topside 监听。leader 协议方向相反。topside 是 follower，必须先发 `Register`。等对端先发 ACP 会卡住，因为 leader 在 `Register` 之前不转发 ACP。

TLS 已经完成、unix 还没连上时，snorkel 会丢掉这次 TLS。topside 会看见短暂连接且没有 `Register`。leader 的 30 秒注册时钟从 unix `accept` 之后开始，不是从 TLS 握手开始。

## Leader 帧

Unix 上每条消息是 4 字节大端长度，后跟 UTF-8 JSON。上限 64MB。读写是 `read_frame` 与 `write_frame`。`LEADER_PROTOCOL_VERSION` 是 `1`。

`ClientMessage` 与 `ServerMessage` 使用 `#[serde(tag = "type", rename_all = "snake_case")]`。变体名在 JSON 里是 `register`、`acp`、`ping`、`pong`、`registered`。字段名保持 Rust 的 snake_case。`ClientCapabilities` 没有 `rename_all`，JSON 键是 `yolo_mode`、`terminal`、`fs_read`，不是 camelCase。

客户端发出的 `Register`：

```json
{
	"type": "register",
	"client_type": "aqualung-topside",
	"mode": "stdio",
	"capabilities": {
		"terminal": false,
		"fs_read": false,
		"fs_write": false
	}
}
```

`mode` 的值是 `stdio` 或 `headless`。`headless` 会走 grok.com relay。远程这条 aqualung 连接用 `stdio`。

服务端在 30 秒内必须先收到 `Register`。否则回 `{"type":"error","code":3,"message":"Registration timeout"}` 并结束会话。第一条不是 `Register` 时回 `{"type":"error","code":1,"message":"Expected Register message"}`。

成功时的 `Registered`：

```json
{
	"type": "registered",
	"client_id": 1,
	"ready": true,
	"leader_protocol_version": 1,
	"leader_binary_version": "unknown",
	"leader_capabilities": {
		"control_v1": true,
		"runtime_cpu_profile": false,
		"profile_formats": [],
		"workspace_exposure": false,
		"relaunch_v1": false
	}
}
```

`ready` 缺省反序列化为 `true`，以兼容旧 leader。`ready` 为 `false` 时，客户端必须等到 `{"type":"leader_ready"}` 再发 ACP。官方 client 等 `LeaderReady` 的时限是 `LEADER_READY_TIMEOUT`。该常量等于 `xai_grok_http::MIN_CLIENT_CONNECT_TIMEOUT`，是 120 秒。官方 client 等 `Registered` 的时限是 10 秒。

ACP 封装。`payload` 是字符串。写成嵌套对象会反序列化失败。

```json
{
	"type": "acp",
	"payload": "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"initialize\",\"params\":{}}"
}
```

官方 client 每 30 秒发 `{"type":"ping"}`。leader 立刻回 `{"type":"pong"}`。leader 不会因为缺 ping 断开。还有 `Control`、`Disconnect`、`ShuttingDown`、`Shutdown`。其余控制命令可以先不发。

## 请求 id 改写

leader 在内部通道上把 follower 请求的 `id` 写成 `{clientId}|{原 id 的 JSON}`。分隔符是 `|`。函数是 `rewrite_request_id`。写回这条 Unix 连接之前会还原。隧道上看到的是 follower 自己的 id。两部手机都用 `id: 1` 时，leader 只看见同一个 follower，内部前缀也撞。手机之间的拆分必须在 topside 做。

只改写带 `method` 且带 `id` 的请求。带 `result` 或 `error` 的响应不改。通知没有 `id`，也不改。

## 会话订阅、先答、掉线

`session_subscribers` 按 `sessionId` 记下哪些 `ClientId` 在看。带 `sessionId` 的 follower 消息会把该 follower 加入订阅。`session/load` 与 `session/resume` 都算挂上，不是挤掉上一个。`session/load` 的历史回放带 `_meta["x.ai/leaderClientId"]`，只回给正在 load 的那个 follower。

权限类反向请求会广播给该场所有订阅者。判定函数 `is_interaction_request` 认这些方法名：

- `session/request_permission`
- `x.ai/ask_user_question`
- `x.ai/exit_plan_mode`
- `x.ai/mcp/elicit`

方法名可能出现在顶层，也可能包在 `_` 前缀的 ext 包装里。`method_of` 会拆开。谁先答谁算发生在 follower 之间。leader 只看见一个 snorkel follower 时，手机之间的先答落在 topside。

不是 interaction 的反向请求只发给 `session_driver`。driver 是该场上第一个发出带 `sessionId` 消息的 follower。

某个 follower 断开后，leader 从订阅表里删掉它。若那场没有剩下的订阅者，leader 向 Agent 发内部通知。线格式是：

```json
{
	"jsonrpc": "2.0",
	"method": "_x.ai/internal/evict_sessions",
	"params": {
		"sessionIds": ["sess_abc"]
	}
}
```

`internal_notification` 把方法名写成 `_{name}`，因为 ACP SDK 只把带 `_` 前缀的自定义方法交给 `ext_notification`。Agent 匹配的名字是去掉前缀后的 `x.ai/internal/evict_sessions`。

最后一个客户端离开时，若没有 `--no-exit-on-disconnect`，leader 进程退出。

新的 snorkel 是新的 unix `accept()`，也就是新的 `ClientId`。leader 不会把新连接当成旧连接的替换。同一时间 topside 只认一条已鉴权的 snorkel。这是 aqualung 的替换规则。替换时若先拆旧 follower，且家里窗口没有订那场，leader 会把那场驱逐掉。

## initialize 与能力注入

作为 follower，topside 仍要向 Agent 发一次自己的 `initialize`。leader 会把 IPC 注册里的 `client_type` 注入到 `initialize` 的 `params._meta.clientIdentifier`。Agent 的 `client_type` 取这一次 initialize。

`session/new`、`session/load`、`session/resume` 上，leader 按该 follower 的 `ClientCapabilities` 注入 `_meta`。函数是 `inject_session_request_context`。`yoloMode` 和 `modelId` 只出现在 `session/new`。其余键在这三种方法上都会写。`yoloMode`、`autoMode`、`modelId`、`clientIdentifier`、`x.ai/leaderClientId` 只在键不存在时写入。`clientTerminal`、`clientFsRead`、`clientFsWrite`、`codeNavEnabled`、`clientStatusLine` 每次都按当前能力重写。

| 能力字段 | 注入的 `_meta` 键 | 为 true 时的后果 |
| --- | --- | --- |
| `terminal` | `clientTerminal` | Agent 把终端 ACP 打到这个客户端 |
| `fs_read` | `clientFsRead` | Agent 把读文件打到这个客户端 |
| `fs_write` | `clientFsWrite` | Agent 把写文件打到这个客户端 |
| `yolo_mode` | `yoloMode` | 自动批准工具。只在 `session/new` 上注入 |
| `auto_mode` | `autoMode` | 非 yolo 时自动许可工具 |
| `status_line` | `clientStatusLine` | Agent 为这个客户端组状态行 payload |
| `code_nav_enabled` | `codeNavEnabled` | Agent 按这个客户端开关 code-nav |
| `default_model` | `modelId` | 只在 `session/new` 上注入 |

同一函数还会写入 `clientIdentifier`（来自 Register 的 `client_type`）和 `x.ai/leaderClientId`（这个 follower 的 `ClientId`）。`session/load` 的历史回放靠后一个键做单播。

`ClientCapabilities` 还有 `client_version`。leader 用它打版本不一致的日志，不注入 `_meta`。

手机路径上 `terminal`、`fs_read`、`fs_write` 为 false。工具仍在家里跑。`yolo_mode`、`auto_mode`、`status_line`、`code_nav_enabled`、`default_model` 在手机路径上取什么值，本页未指定。

## grok agent serve 不是这条路

`crates/codegen/xai-grok-shell/src/agent/server.rs` 是 `grok agent serve` 的 WebSocket。它用 `Authorization: Bearer` 或查询参数 `server-key`。它把每个文本帧去掉行尾换行，再补一个 `\n` 喂给 ACP SDK 的行协议。`relay_dest` 一次只指向当前那条 WebSocket。新连接把出站通知改接到新客户端。旧的收不到。

这不是 `leader.sock` 上的多 follower 路由。aqualung 不走这条路。

Grok 工作区依赖 `agent-client-protocol = "0.10.4"`，feature `unstable`。这与 crates.io 上当前的 2.0.0 不是同一代 API。

## 7678 上的手机 ACP

对手机，topside 是 ACP Agent。手机是 Client。

验证客户端 `.cursor/skills/verify-aqualung/bin/phone.py` 的握手是：

- 明文 TCP 到 `127.0.0.1:7678`
- `GET / HTTP/1.1`
- `Upgrade: websocket`
- `Sec-WebSocket-Version: 13`
- `Authorization: Bearer <token>`
- 没有 `Sec-WebSocket-Protocol`
- 期望 `101`
- 发送一个 WebSocket 文本帧，内含一个 JSON-RPC 对象
- 读取一个文本帧

官方 ACP v1 传输是 stdio，消息按换行分隔且不得内嵌换行。[Streamable HTTP 与 WebSocket RFD](https://agentclientprotocol.com/rfds/streamable-http-websocket-transport.md) 提议同一 `/acp` 端点上的 GET 升级。RFD 不是已稳定的 v1 传输。aqualung 验证图已经选定 7678、`GET /`、一帧一个 JSON 对象。那是 aqualung 的自定义传输，不是 RFD 的 `/acp`。

一条 ACP 连接上可以同时有多场会话。所有文件路径必须是绝对路径。行号从 1 起。

ACP v1 协议页里 Client 发给 Agent 的基线方法是 `initialize`、`authenticate`、`session/new`、`session/prompt`。可选方法是 `session/load`、`session/resume`、`session/close`、`session/delete`、`session/list`、`session/set_mode`、`session/set_config_option`、`logout`。通知是 `session/cancel`。可选通知 `$/cancel_request` 带 `requestId`。

Agent 发给 Client 的基线方法是 `session/request_permission`。可选方法是 `fs/read_text_file`、`fs/write_text_file`、`terminal/create`、`terminal/output`、`terminal/wait_for_exit`、`terminal/kill`、`terminal/release`、`elicitation/create`。通知是 `session/update` 与 `elicitation/complete`。这些方法名和下面的错误码来自 ACP v1 协议页，不是来自 SDK 钉死的 schema 1.5.0。

`initialize` 必须在任何 session 方法之前。请求带整数 `protocolVersion` 与 `clientCapabilities`。响应带选定的 `protocolVersion`、`agentCapabilities`、`authMethods`。省略的能力视为不支持。`authMethods` 的 schema 缺省是 `[]`。初始化示例用空数组，然后进入 session。v1 没有另写空数组的含义。Client 因此没有可传给 `authenticate` 的 `methodId`。传输层 bearer 不是 ACP `authenticate`。`session/new` 在需要认证时仍可以回 `auth_required`。schema 里该错误码是 `-32000`。v1 没有 `initialized` 通知。那是 RFD 的词。`phone.py` 只做一次 `initialize` 往返。通知 `$/cancel_request` 对应 `-32800` Request cancelled。普通 JSON-RPC 码是 `-32700`、`-32600`、`-32601`、`-32602`、`-32603`。资源未找到是 `-32002`。

`session/request_permission` 是权限弹窗的 ACP 方法名。取消进行中的 prompt 时，Client 必须对未完成的权限请求回 `cancelled`。

JSON-RPC 2.0 的 `id` 可以是字符串或数字。通知没有 `id`。schema 里的 `RequestId` 还有 `Null`，用于解析失败时的响应。ACP 对象键是 camelCase。判别字段的字符串值是 snake_case。

crates.io 上 `agent-client-protocol` 2.0.0 的许可证是 Apache-2.0。它精确钉死 `agent-client-protocol-schema =1.5.0`。crates.io 上 schema 的最新版是 1.7.0。MSRV 是 1.88.0。文档里的主类型是 `Builder`、`ByteStreams`、`Lines`、`Stdio`。`Agent` 是角色类型，不是要 impl 的 trait。官方库页面仍写 impl `Agent` trait，并指向已删除的 `examples/agent.rs`。未注册的请求得到 JSON-RPC method not found。

`agent-client-protocol-http` 2.0.0 默认 feature 为空，服务端要开 `server`。默认路径是 `/acp`，不是 `phone.py` 的 `GET /`。`ServerOptions` 没有 bearer 字段。WebSocket 文本帧解析成一个 JSON-RPC 对象或一个非空 batch 数组。同一帧里拼接多个对象会解析失败。`Stdio` 与 `ByteStreams` 会在每条消息后加换行。那与验证图的一帧一个对象不是同一套成帧。

`tokio-tungstenite` 的 `accept_hdr_async` 和 axum 的 upgrade 提取器都能在发出 101 之前读到握手 GET 上的 `Authorization`。`agent-client-protocol-http` 的客户端连接不带自定义头。

ACP v2 仍是 draft。

## 本页未指定的事项

这些在已引来源里找不到，因此不是事实。

- `topside` 的 CLI 标志与环境变量名
- 7678 是否由 topside 做 TLS，或由反向代理做 TLS
- host-away 的 JSON-RPC 方法名、是通知还是错误、清除时是否再发一条
- 一部 topside 上是一张 bearer token 还是多张
- 两部手机同时 `session/prompt` 时 topside 或 Agent 的排队、拒绝、忙闸
- Register 里 `yolo_mode`、`auto_mode`、`status_line`、`code_nav_enabled`、`default_model` 取什么值
- 手机 `initialize` 在家里离线时返回的 `agentCapabilities` 具体字段
- `session/load` 与 `fs/write_text_file` 的 `result` 是 `null` 还是对象。文档示例与 schema 不一致
- topside 用 SDK 2.0、Grok 钉死的 0.10.4，还是手写 JSON-RPC
