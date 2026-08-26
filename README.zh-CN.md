# aqualung

[English](README.md) | [简体中文](README.zh-CN.md)

从手机上使用家里机器上的编程代理。

aqualung 由两个小程序组成。`snorkel` 在家里和代理一起运行，向外拨号连到你自己控制的服务器。`topside` 跑在那台服务器上，对手机客户端讲 [Agent Client Protocol](https://agentclientprotocol.com)。代理、它的工具、以及对话，都留在家里那台机器上。

## 状态

`snorkel` 已经存在。`topside` 还没有实现。接口仍不稳定。

## 在家里的机器上运行 snorkel

先运行 `cargo build -p snorkel`，再把 agent 的 unix socket 和 topside 的
mTLS 监听地址传给它：

```sh
target/debug/snorkel \
  --socket /path/to/agent.sock \
  --server topside.example.com \
  --cert /path/to/client.pem \
  --key /path/to/client-key.pem \
  --ca /path/to/ca.pem
```

服务器端口默认是 1943。这五个值也可以分别通过 `SNORKEL_SOCKET`、
`SNORKEL_SERVER`、`SNORKEL_CERT`、`SNORKEL_KEY` 和 `SNORKEL_CA` 提供。
加上 `--once` 后，第一次连接尝试或会话结束时进程会退出，不再重连。

## 工作原理

家里那台机器跑代理。代理在本地 unix socket 上监听，编辑器和终端连上去。`snorkel` 连上这个 socket，再通过 TLS 拨到服务器。它只做双向拷贝字节，别的什么都不做：不解析数据流，也不讲 ACP。拨号、客户端证书、断线重连，就是它的全部工作。

`topside` 在服务器上终结这条流。对代理来说，它是一个远程客户端。对手机来说，它是 ACP 服务器，并把多部手机复用到这一条连接上：它会改写 JSON-RPC 的请求 id，把会话更新扇出给所有正在看这个会话的手机，自己应答 `initialize` 和认证。它同一时间只服务一个 `snorkel`；新认证的连接会顶掉旧的，这样家里卡住的进程就不会把你锁在外面。

`topside` 状态只放在内存里，不写磁盘。它重启之后，手机会重新加载会话再连上。会话本身只存在于家里。

你在家里用的编辑器或终端直接连本地 socket，不经过服务器。`topside` 挂了，家里的工作不受影响。家里那台机器离线时，手机会被告知主机不在。

`snorkel` 用双向 TLS 拨到服务器的 1943/tcp；服务器只信任一张客户端证书。手机连 7678/tcp，用 bearer token 认证，在 WebSocket 上跑 ACP，每条消息一个 JSON-RPC 对象。

## 非目标

aqualung 不是通用隧道。要暴露任意 TCP 端口，请用 [rathole](https://github.com/rathole-org/rathole) 或 [frp](https://github.com/fatedier/frp)。

`topside` 不跑工具、不读文件、也不开终端，更不会替手机注册这些能力。它不存对话、不存工具输出、也不存会话记录。它也从不主动拨回家：两台机器之间只有 `snorkel` 向外开的那一条连接，家里那台机器不需要入站端口，也不需要端口转发。

## 名字

1943 年 Cousteau 和 Gagnan 给 Aqua-Lung 调节器申请了专利，端口号就来自这个年份。`snorkel` 是从水下伸上来的管子。`topside` 是水面那一端，船员站在那里，不下潜。

## 贡献

设计还在变。写代码之前请先开 issue 或 discussion。

## 许可证

Apache License 2.0。见 [LICENSE](LICENSE)。
