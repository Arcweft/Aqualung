# Snorkel 接到 Grok leader

家里跑 Grok Build。手机经 aqualung 用 ACP 操作同一份 Agent。本文说明 leader 在线上是什么，以及 snorkel 能不能只拷字节就接上去。

对照的源码是 `/Users/doby/Chat/repos/grok-build`，`SOURCE_REV` `28439e8a8712c363321cf6ff0c2d70cd058d2a7d`。本机安装的二进制是 `grok 1.0.10`。

## 结论

能落地。snorkel 接的是 `~/.grok/leader.sock` 上**一条**已接受的 Unix 连接，不是 ACP 行协议，也不是 `grok agent serve`。

snorkel 仍然只做拨号、mTLS、双向拷贝、断线重连。它不 Register，不拆帧，不讲 ACP。

topside 才是 leader 的 follower。它对手机讲 ACP，对这条出站连接讲 leader 私有协议。多部手机复用这一个 follower。家里的 `grok` 窗口是并列的另一个 follower，走同一颗 socket 文件上的另一条连接。

家里要常驻 `grok agent leader --no-exit-on-disconnect`。`[cli] use_leader` 打开时，家里窗口当 follower，不要再给每部手机起一份 `grok agent --leader stdio`。

## 会改变什么

对手机用户。外出时连你的 VPS，用 ACP 看家里 Mini 上的同一场会话。家里窗口可以同时开着。Mini 不听入站端口。

对接下来写 aqualung 的人。home 侧的“代理 socket”在 Grok 上就是 `leader.sock`。README 里“拷贝字节、不讲 ACP”仍然成立。不成立的是“那条流本身是 JSON-RPC”。JSON-RPC 包在 leader 帧的 `Acp.payload` 里。id 改写、会话扇出、权限先答，leader 只在 **follower** 之间做。手机之间的同样工作必须在 topside 做，因为 leader 只看见一个 ClientId。

## Leader 协议

一份 Agent，挂在一个 leader 进程里。多个客户端连 `leader.sock`。架构说明在 `crates/codegen/xai-grok-shell/src/leader/mod.rs`。

**传输。** Unix 上是 `UnixStream`（`leader/transport.rs`）。每条消息是 4 字节大端长度，后跟 JSON，上限 64MB（`protocol.rs` 的 `read_frame` / `write_frame`）。`LEADER_PROTOCOL_VERSION` 现在是 `1`。

**握手。** 客户端先发 `ClientMessage::Register`，带 `client_type`、`mode`、`capabilities`。服务端回 `Registered { client_id, ready, ... }`。`ready == false` 时，客户端必须等到 `LeaderReady` 再发 ACP（`client.rs` 的 `register`）。远程这条连接用 `ClientMode::Stdio`。`Headless` 会向 grok.com relay 要流量，不要用。

**ACP 怎么走。** 客户端 `{"type":"acp","payload":"<json-rpc 字符串>"}`。服务端同形。leader 在进 Agent 之前把请求 id 写成 `{clientId}|{原 id 的 JSON}`（`server.rs` 的 `rewrite_request_id`，分隔符 `|`）。回包再拆开还给对应 follower。

**会话。** `session_subscribers` 按 `sessionId` 记哪些 `ClientId` 在看。`session/update` 只发给订阅了那场的人，不是全量广播。`session/load` 和 `session/resume` 是再挂一块屏幕，不是挤掉上一个，也不是隐式 fork。`session/load` 的历史回放只给正在 load 的那个 follower（`x.ai/leaderClientId`）。权限弹窗、`ask_user_question`、`exit_plan_mode`、`mcp/elicit` 会广播给该场所有订阅者，谁先答谁算（`is_interaction_request`）。

**掉线。** 某个 follower 断开后，leader 从订阅表里删掉它。若那场没有剩下的订阅者，leader 向 Agent 发 `x.ai/internal/evict_sessions`。最后一个客户端离开时，若没有 `--no-exit-on-disconnect`，leader 进程自己退出。

**控制面。** 还有 `Ping`/`Pong`、`Control`（leader 信息、CPU profile、workspace、为升级而 relaunch）。snorkel 原样拷这些字节。topside 至少要能握手、转发 ACP、对 `Ping` 回 `Pong`。其余控制命令可以先不发。

## Snorkel 怎么接

```
手机  -- ACP / WebSocket / 7678 -->  topside
topside  -- leader 帧 / mTLS / 1943 -->  snorkel
snorkel  -- 原样字节 -->  ~/.grok/leader.sock
家里 grok 窗口  -- leader 帧 -->  同一 socket 文件上的另一条连接
```

snorkel 在家里 `connect()` 那颗 socket，把得到的那条流和 VPS 上的 mTLS 对拷。新的 snorkel 是新的 `accept()`，也就是新的 follower。旧的那条断了，leader 只丢掉旧的 `ClientId`，不会把新的当成“替换同一个连接”。

同一时间 topside 只认一条已鉴权的 snorkel。这是 aqualung 的替换规则，不是 leader 的。leader 本来就允许多个 follower 并存。

## Topside 必须做的事

这些不能下放到 snorkel，也不能指望 leader 替手机做。

1. **讲 leader 方言。** 连上后 `Register`（`mode: stdio`）。`Registered.ready == false` 就等到 `LeaderReady`。之后把手机的 JSON-RPC 放进 `Acp.payload`，把对端的 `Acp.payload` 拆回 JSON-RPC。
2. **自己应答 `initialize` 和认证。** 这是 aqualung 已写明的。作为 follower，topside 仍要向 Agent 发一次自己的 `initialize`。手机各自的 `initialize` 不要原样灌进 leader。
3. **改写手机请求 id。** leader 只按这一个 `ClientId` 加前缀。两部手机都用 `id: 1` 会在同一条 follower 上撞车。扇出前必须先在 topside 命名空间里拆开。
4. **按 `sessionId` 扇出给手机。** leader 只会把更新发给这个唯一的 snorkel follower。看 s1 的手机 A 和看 s2 的手机 B，分发在 topside。
5. **手机之间的权限先答。** leader 的先答只发生在 TUI 和 snorkel 之间。多部手机抢同一弹窗，topside 收齐、先答者胜、只回一条给 leader。
6. **能力不要替家里代报。** `ClientCapabilities` 里的 `terminal` / `fs_read` / `fs_write` 为 true 时，leader 会让 Agent 把终端和文件打到这个客户端。手机路径上这些应为 false。工具仍在 Mini 上跑。
7. **host-away。** snorkel 或 leader 断开时，7678 上的 WebSocket 保持，向手机发主机不在。不要把 7678 直接掐掉。
8. **替换时先订阅再拆旧连接。** 见下一节。

## 替换顺序

snorkel 卡住再拉起来时，新连接鉴权成功就会顶掉旧的。若旧 follower 先断，且家里窗口没有订那场，leader 会把那场 `EvictSessions` 掉。手机再 `session/load` 会空。

正确顺序：

1. 新 snorkel 的 mTLS 握手成功。
2. topside 在新流上 `Register`，并对仍有人看的 `sessionId` 做 `session/load`。
3. 新 follower 已经出现在 `session_subscribers` 里之后，再拆旧 mTLS。

旧 snorkel 在第 3 步之前仍连着 `leader.sock`。两份 snorkel 短暂并存是允许的。leader 把它们当成两个 follower。

家里窗口若已经订了同一场，即使顺序写错，那场也不会被赶走。不要依赖这一点。手机新开的场往往只有 snorkel 在订。

## 不采用的接入点

**`grok agent serve`。** 默认听 `127.0.0.1:2419`。家里要入站，或者再套一层隧道。`agent/server.rs` 里一份 Agent 跨重连复用，但 `relay_dest` 一次只指向当前那条 WebSocket。新连接把出站通知改接到新客户端，旧的收不到。serve 不会把 `leader.sock` 上的多 follower 路由暴露出去。

**`grok agent --leader stdio` 当远程翻译器。** 会把私有帧变成公有 ACP，topside 可以不跟 grok 升版本。代价是 Mini 上多一个进程，而且拷的不是编辑器连的那条 socket。aqualung 的 home bypass 要求本地编辑器直连 `leader.sock`。多一个 stdio 适配器不是那条路。

**Cloudflare Tunnel 或把 Mini 暴露成源站。** 违反“只出站、不听端口”。

**给每部手机在 Mini 上起一个 follower。** snorkel 不解析、一条 TCP。多路复用在 topside，不在家里。

## 版本耦合

topside 跟的是 grok 的 leader 方言，不是 ACP 规范本身。`Register` 形状、帧头、`ready`/`LeaderReady`、id 前缀、内部通知名字，升 grok 都可能变。`protocol.rs` 写明新字段必须 `#[serde(default)]`，新旧二进制可以混跑，但这不是承诺线格式稳定。

topside 记下 `Registered` 里的 `leader_protocol_version` 和 `leader_binary_version`。对不上就拒绝这条 snorkel，让手机看到明确错误，不要在拆帧失败时保持沉默。

## 还不知道的事

- 本仓库还没有 `snorkel` / `topside` 二进制。`control-aqualung doctor` 现在退出 2。下面这些要等有进程才能用运行时证明。
- topside 是否要周期发 `Ping`，leader 多久把静默连接踢掉。代码里有 `Ping`/`Pong`，超时策略没有在这一次调研里钉死。
- `session/prompt` 在途时，另一部手机再 prompt，Agent 侧是排队还是拒绝。aqualung 验证图写的是 topside 立刻回忙。需要对照 Agent 的 prompt 队列再定，不要先假设 leader 会挡。
- Windows named pipe 不在范围内。Mini 是 macOS。

## 代码锚点

| 主题 | 位置 |
| --- | --- |
| 帧、Register、消息枚举 | `crates/codegen/xai-grok-shell/src/leader/protocol.rs` |
| Unix 传输 | `crates/codegen/xai-grok-shell/src/leader/transport.rs` |
| 握手与 LeaderReady | `crates/codegen/xai-grok-shell/src/leader/client.rs` |
| 路由、id 前缀、订阅、先答、掉线驱逐 | `crates/codegen/xai-grok-shell/src/leader/server.rs` |
| 锁与 `GROK_LEADER_SOCKET` | `crates/codegen/xai-grok-shell/src/leader/lock.rs` |
| 总览 | `crates/codegen/xai-grok-shell/src/leader/mod.rs` |
| `grok agent serve` 单槽 WebSocket | `crates/codegen/xai-grok-shell/src/agent/server.rs` |
| 公开 ACP / stdio / serve | `crates/codegen/xai-grok-pager/docs/user-guide/15-agent-mode.md` |
| 两客户端同场 | `crates/codegen/xai-grok-pager/tests/leader_pty_e2e/leader_two_clients_shared_session.rs` |

aqualung 侧已写明、且本设计遵守的约束，见仓库根目录 `README.md`，以及 `.cursor/skills/verify-aqualung/features/` 下的 phone-attach、host-away、snorkel-replace、session-fanout、home-bypass。
