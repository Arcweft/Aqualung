# Session load

Session load is a phone re-opening a session that already exists on the home agent. topside forwards `session/new` and `session/load` through snorkel onto `leader.sock`. It does not mint a `sessionId`.

## Sub-features

- `load-after-new` creates a session with `session/new`, then `session/load` that id on the same 7678 WebSocket.
- `load-needs-home` returns `host is away` until the GHCR home side has Registered and finished follower initialize.

## How to get to it (user POV)

- Attach a phone to topside, create a session, then load it again. After a topside restart the phone loads the same home session.

## Driving it with control-aqualung

Preconditions:

- `control-aqualung launch-home` or `control-aqualung launch-image` has been run for this `AQUALUNG_VERIFY_RUN`. Local `launch` points snorkel at a missing unix socket. That path cannot forward session methods. Record it as unreachable.
- `control-aqualung doctor --save` exits 0. Doctor names `"launch": "home"` or `"launch": "docker"`.
- The token is `run/$AQUALUNG_VERIFY_RUN/token`.

- **Mux is up.** Run `control-aqualung session-load`. It retries while `session/new` is `host is away`.
- **Same connection.** The command uses one WebSocket: `initialize`, then `session/new` with `cwd` `/tmp` and `mcpServers` `[]`, then `session/load` of the returned `sessionId` with the same `cwd` and `mcpServers`.
- **Home answered.** Exit 0. `artifacts/$AQUALUNG_VERIFY_RUN/session-load/probe.ndjson` has a `session/new` result with `sessionId` and a `session/load` reply that is not `host is away` and not `phone has not initialized`. A lone `initialize` result is not this feature.
- **No sessionId.** Exit 3 means grok answered `session/new` without a `sessionId` (often auth). Save the transcript. That is not a pass. It is also not host-away.
- **Proof.** Save `probe.ndjson`, `probe.cmd.txt`, and `doctor.json`. The entry point is 7678 with a bearer token.

## Gotchas

- Phone `initialize` is answered by topside. It works with no home agent. Do not treat it as `session/load`.
- `leader.client.registered` in grok logs proves Register, not `session/load`.
- First attach does not make Hub send `session/load` by itself. The phone must send it.
- `control-aqualung phone` with one `--send` still closes after one reply. Use `session-load` or several `--send` on one invocation.
- Empty `GROK_HOME` has no `auth.json`. `launch-image` and `docker/smoke.sh` set a dummy `XAI_API_KEY` and pin `[auth] preferred_method = "api_key"` so grok selects a method without probing the live API. `session/new` then mints a local `sessionId`. `launch-home` uses host grok auth instead. `session/prompt` is not this feature and will fail with that dummy.
- topside answers phone `authenticate` itself (`-32601`). It does not log the follower into grok. The dummy is for the Agent process, not for 7678.
