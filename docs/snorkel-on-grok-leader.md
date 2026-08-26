# Snorkel 与 Grok leader

家里跑 Grok Build。手机经 aqualung 用 ACP 操作同一份 Agent。你接下来要实现 `snorkel` 和 `topside`。

snorkel 接的是 `~/.grok/leader.sock` 上一条已经 accept 的 Unix 连接。那条流是 leader 私有帧，不是 ACP 行协议，也不是 `grok agent serve`。

snorkel 只做拨号、mTLS、双向拷贝、断线重连。它不 Register，不拆帧，不讲 ACP。topside 才是 leader 的 follower。它对手机讲 ACP，对这条出站连接讲 leader 协议。多部手机复用这一个 follower。家里的 `grok` 窗口是并列的另一个 follower，走同一颗 socket 文件上的另一条连接。

家里常驻 `grok agent leader --no-exit-on-disconnect`。`[cli] use_leader` 打开时，家里窗口当 follower。每部手机再起一份 `grok agent --leader stdio` 不在这条设计里。

外出时你连自己的 VPS，用 ACP 看家里机器上的同一场会话。家里窗口可以同时开着。家里的机器不听入站端口。

家里那颗代理 socket 在 Grok 上就是 `leader.sock`。[README](../README.md) 里「拷贝字节、不讲 ACP」仍然成立。不成立的是「那条流本身是 JSON-RPC」。JSON-RPC 是 `Acp.payload` 里的字符串，不是嵌套对象。会话扇出和权限先答，leader 只在 follower 之间做。手机之间的同样工作必须在 topside 做，因为 leader 只看见一个 `ClientId`。

## 拓扑

```
phone  -- ACP over WebSocket on 7678 -->  topside
topside  -- leader frames over mTLS on 1943 -->  snorkel
snorkel  -- raw bytes -->  ~/.grok/leader.sock
home grok window  -- leader frames -->  same socket file, another connection
```

snorkel 在家里 `connect()` 那颗 socket，把得到的那条流和 VPS 上的 mTLS 对拷。新的 snorkel 是新的 `accept()`，也就是新的 follower。旧的那条断了，leader 只丢掉旧的 `ClientId`。它不会把新连接当成替换同一个连接。

同一时间 topside 只认一条已鉴权的 snorkel。这是 aqualung 的替换规则，不是 leader 的。leader 允许多个 follower 并存。

## 连接上的协议

一份 Agent 挂在一个 leader 进程里。多个客户端连 `leader.sock`。

### 帧

Unix 上传输是 `UnixStream`。每条消息是 4 字节大端长度，后跟 JSON，上限 64MB。读写在 `read_frame` 和 `write_frame`。`LEADER_PROTOCOL_VERSION` 现在是 `1`。

### 握手

客户端必须在 30 秒内先发 `ClientMessage::Register`，带上 `client_type`、`mode`、`capabilities`。否则 leader 回 `Registration timeout` 并断开。服务端回 `Registered`，其中有 `client_id` 和 `ready`。`ready` 为 false 时，客户端等到 `LeaderReady` 再发 ACP。

远程这条连接用 `ClientMode::Stdio`。`Headless` 会向 grok.com relay 要流量，这条路径不用。socket 上没有 token，也没有 peer cred。能 `connect()` 的本地进程就是 client。

### ACP 封装

客户端发送 `{"type":"acp","payload":"<json-rpc 字符串>"}`。服务端同形。`payload` 必须是字符串。写成嵌套对象会直接反序列化失败。

leader 在内部通道上把请求 id 写成 `{clientId}|{原 id 的 JSON}`，分隔符是 `|`，函数是 `rewrite_request_id`。写回这条 Unix 连接之前会还原。隧道上看到的是 follower 自己的 id，看不到 `123|42`。

### 会话

`session_subscribers` 按 `sessionId` 记哪些 `ClientId` 在看。`session/update` 只发给订阅了那场的人，不是全量广播。`session/load` 和 `session/resume` 是再挂一块屏幕，不是挤掉上一个，也不是隐式 fork。`session/load` 的历史回放只给正在 load 的那个 follower，标记是 `x.ai/leaderClientId`。

权限弹窗、`ask_user_question`、`exit_plan_mode`、`mcp/elicit` 会广播给该场所有订阅者。谁先答谁算，判定在 `is_interaction_request`。

### 掉线

某个 follower 断开后，leader 从订阅表里删掉它。若那场没有剩下的订阅者，leader 向 Agent 发 `x.ai/internal/evict_sessions`。最后一个客户端离开时，若没有 `--no-exit-on-disconnect`，leader 进程自己退出。

### 控制帧

控制帧是 `Ping`、`Pong`、`Control`。`Control` 覆盖 leader 信息、CPU profile、workspace、为升级而 relaunch。snorkel 原样拷这些字节。官方 client 每 30 秒发 `Ping`。leader 立刻回 `Pong`，不会因为缺 ping 把连接踢掉。topside 按 `type` 把这些帧从 ACP 里分出来。它不能把整帧当 JSON-RPC。其余控制命令可以先不发。

## topside 为什么是 follower

leader 看不见单部手机。下面的工作落在 topside。

topside 连上后发出 `Register`，`mode` 为 `stdio`。`Registered.ready` 为 false 时，它等到 `LeaderReady`。之后把手机的 JSON-RPC 放进 `Acp.payload`，把对端的 `Acp.payload` 拆回 JSON-RPC。

`initialize` 和认证仍由 topside 应答。这是 aqualung 已写明的。作为 follower，topside 仍要向 Agent 发一次自己的 `initialize`。Agent 只吃第一次 `initialize`，后来的会被丢掉。手机各自的 `initialize` 不会原样灌进 leader。

两部手机都用 `id: 1` 时，leader 只看见同一个 follower，内部前缀也撞。隧道上不会出现 `|` 前缀。扇出前 topside 先在自己的命名空间里拆开请求 id。

leader 只会把更新发给这个唯一的 snorkel follower。看 s1 的手机 A 和看 s2 的手机 B，分发在 topside。

leader 的先答只发生在 TUI 和 snorkel 之间。多部手机抢同一弹窗时，topside 收齐、先答者胜、只回一条给 leader。

`ClientCapabilities` 里的 `terminal`、`fs_read`、`fs_write` 为 true 时，leader 会让 Agent 把终端和文件打到这个客户端。手机路径上这些为 false。工具仍在家里的机器上跑。

snorkel 或 leader 断开时，7678 上的 WebSocket 保持，向手机发主机不在。7678 不随 home 掉线一起关掉。

## 为什么替换顺序不能反

snorkel 卡住再拉起来时，新连接鉴权成功就会顶掉旧的。若旧 follower 先断，且家里窗口没有订那场，leader 会把那场 `EvictSessions` 掉。手机再 `session/load` 会空。

设计要求的顺序是：

1. 新 snorkel 的 mTLS 握手成功。
2. topside 在新流上 `Register`，并对仍有人看的 `sessionId` 做 `session/load`。
3. 新 follower 已经出现在 `session_subscribers` 里之后，再拆旧 mTLS。

旧 snorkel 在第 3 步之前仍连着 `leader.sock`。两份 snorkel 短暂并存是允许的。leader 把它们当成两个 follower。

家里窗口若已经订了同一场，即使顺序写错，那场也不会被赶走。家里窗口的订阅不是可依赖的保证。手机新开的场往往只有 snorkel 在订。

## 拒绝的接入点

`grok agent serve` 默认听 `127.0.0.1:2419`。家里要入站，或者再套一层隧道。一份 Agent 跨重连复用，但 `relay_dest` 一次只指向当前那条 WebSocket。新连接把出站通知改接到新客户端，旧的收不到。serve 不会把 `leader.sock` 上的多 follower 路由暴露出去。

`grok agent --leader stdio` 当远程翻译器，会把私有帧变成公有 ACP。topside 可以不跟 grok 升版本。代价是家里多一个进程，而且拷的不是编辑器连的那条 socket。aqualung 的 home bypass 要求本地编辑器直连 `leader.sock`。多一个 stdio 适配器不是那条路。

Cloudflare Tunnel，或把家里的机器暴露成源站，违反「只出站、不听端口」。

给每部手机在家里起一个 follower 也不采用。snorkel 不解析、一条 TCP。多路复用在 topside，不在家里。

## 版本跟的是 leader 方言

topside 跟的是 grok 的 leader 方言，不是 ACP 规范本身。`Register` 形状、帧头、`ready` 与 `LeaderReady`、内部通知名字，升 grok 都可能变。协议里新字段必须 `#[serde(default)]`，新旧二进制可以混跑。这不是承诺线格式稳定。

topside 记下 `Registered` 里的 `leader_protocol_version` 和 `leader_binary_version`。对不上就拒绝这条 snorkel，让手机看到明确错误。拆帧失败时若保持沉默，手机会以为连接还活着。

## 尚未证实的事

- 本仓库还没有 `snorkel` 或 `topside` 二进制。`control-aqualung doctor` 现在退出 2。下面这些要等有进程才能用运行时证明。
- `session/prompt` 在途时，另一部手机再 prompt，Agent 侧是排队还是拒绝。aqualung 验证图写的是 topside 立刻回忙。Grok 源码里没有追进 prompt 队列。不要先假设 leader 会挡。
- Windows named pipe 不在范围内。家里的机器是 macOS。

## 相关材料

Grok Build 源码里对过的位置：

| 主题 | 位置 |
| --- | --- |
| 帧、Register、消息枚举 | `crates/codegen/xai-grok-shell/src/leader/protocol.rs` |
| Unix 传输 | `crates/codegen/xai-grok-shell/src/leader/transport.rs` |
| 握手与 LeaderReady | `crates/codegen/xai-grok-shell/src/leader/client.rs` |
| 路由、id 前缀、订阅、先答、掉线驱逐 | `crates/codegen/xai-grok-shell/src/leader/server.rs` |
| 锁与 `GROK_LEADER_SOCKET` | `crates/codegen/xai-grok-shell/src/leader/lock.rs` |
| 总览 | `crates/codegen/xai-grok-shell/src/leader/mod.rs` |
| `grok agent serve` 单槽 WebSocket | `crates/codegen/xai-grok-shell/src/agent/server.rs` |
| 公开 ACP、stdio、serve | `crates/codegen/xai-grok-pager/docs/user-guide/15-agent-mode.md` |
| 两客户端同场 | `crates/codegen/xai-grok-pager/tests/leader_pty_e2e/leader_two_clients_shared_session.rs` |

aqualung 侧已写明、且本设计遵守的约束：

- [README.md](../README.md)
- [Phone attach](../.cursor/skills/verify-aqualung/features/phone-attach.md)
- [Host away](../.cursor/skills/verify-aqualung/features/host-away.md)
- [Snorkel replace](../.cursor/skills/verify-aqualung/features/snorkel-replace.md)
- [Session fan-out](../.cursor/skills/verify-aqualung/features/session-fanout.md)
- [Home bypass](../.cursor/skills/verify-aqualung/features/home-bypass.md)
