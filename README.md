# aqualung

[English](README.md) | [简体中文](README.zh-CN.md)

Use the coding agent on your home machine from your phone.

aqualung is two small programs. `snorkel` runs next to the agent at home and dials
out to a server you control. `topside` runs on that server and speaks the
[Agent Client Protocol](https://agentclientprotocol.com) to phone clients. The
agent, its tools, and its conversations stay on the machine at home.

## Status

`snorkel` exists. `topside` does not. The interface is not stable.

## Run snorkel at home

Build the binary with `cargo build -p snorkel`, then point it at the agent's unix
socket and topside's mTLS listener:

```sh
target/debug/snorkel \
  --socket /path/to/agent.sock \
  --server topside.example.com \
  --cert /path/to/client.pem \
  --key /path/to/client-key.pem \
  --ca /path/to/ca.pem
```

The server port defaults to 1943. The five values can also come from
`SNORKEL_SOCKET`, `SNORKEL_SERVER`, `SNORKEL_CERT`, `SNORKEL_KEY`, and
`SNORKEL_CA`. Add `--once` to stop after the first connection attempt or session
instead of reconnecting.

## How it works

The machine at home runs the agent, which listens on a local unix socket for the
editors and terminals attached to it. `snorkel` connects to that socket and dials
the server over TLS. It copies bytes in both directions and does nothing else: it
does not parse the stream and it does not speak ACP. Dialing, the client
certificate, and reconnection are its whole job.

`topside` terminates that stream on the server. To the agent it is a single remote
client. To phones it is an ACP server, and it multiplexes several of them onto the
one connection: it rewrites JSON-RPC request ids, fans session updates out to
every phone watching that session, and answers `initialize` and authentication
itself. It serves one `snorkel` at a time; a newly authenticated connection
replaces the old one, so a wedged process at home cannot lock you out.

`topside` keeps its state in memory and writes nothing to disk. When it restarts,
phones re-attach by loading the session again. The session itself only ever exists
at home.

The editor or terminal you use at home connects to the local socket directly and
does not go through the server. If `topside` dies, work at home is unaffected. If
the machine at home is offline, phones are told the host is away.

`snorkel` dials the server on 1943/tcp with mutual TLS; the server trusts exactly
one client certificate. Phones connect on 7678/tcp, authenticate with a bearer
token, and carry ACP over WebSocket, one JSON-RPC object per message.

## Non-goals

aqualung is not a general-purpose tunnel. To expose arbitrary TCP ports, use
[rathole](https://github.com/rathole-org/rathole) or
[frp](https://github.com/fatedier/frp) instead.

`topside` does not run tools, read files, or open terminals, and it never registers
those capabilities on behalf of a phone. It stores no conversations, no tool
output, and no transcripts. It also never dials home: the only connection between
the two hosts is the one `snorkel` opens outbound, and the machine at home needs no
inbound ports and no port forwarding.

## Name

Cousteau and Gagnan patented the Aqua-Lung regulator in 1943, which is where the
port number comes from. `snorkel` is the tube from below. `topside` is the surface
end, where the crew stands and does not dive.

## Contributing

The design is still moving. Please open an issue or a discussion before writing
code against it.

## License

Apache License 2.0. See [LICENSE](LICENSE).
