# Session prompt

Session prompt is a phone sending one turn to the home agent. topside forwards `session/new` and `session/prompt` through snorkel onto `leader.sock`. The model runs at home.

## Sub-features

- `prompt-after-new` creates a session with `session/new`, then `session/prompt` on the same 7678 WebSocket.
- `prompt-needs-home` returns `host is away` until home has Registered. Dummy GHCR credentials are not this feature.

## How to get to it (user POV)

- Attach a phone to topside and send a prompt to the home agent.

## Driving it with control-aqualung

Preconditions:

- `control-aqualung launch-home` has been run for this `AQUALUNG_VERIFY_RUN`. Local `launch` points snorkel at a missing unix socket. `launch-image` uses a dummy key that cannot call the model. Record those as unreachable.
- `control-aqualung doctor --save` exits 0. Doctor names `"launch": "home"`.
- The token is `run/$AQUALUNG_VERIFY_RUN/token`. grok auth is the host `GROK_HOME` (default `~/.grok`).

- **Mux is up.** Run `control-aqualung session-prompt`. It retries while `session/new` is `host is away`.
- **Same connection.** The command uses one WebSocket: `initialize`, then `session/new` with `cwd` `/tmp` and `mcpServers` `[]`, then `session/prompt` of the returned `sessionId`. Default text is `Reply with the single word PONG and nothing else.` Override with `--text`.
- **Home answered.** Exit 0. `artifacts/$AQUALUNG_VERIFY_RUN/session-prompt/probe.ndjson` has a `session/prompt` result with `stopReason`. Notifications such as `session/update` may be in `probe.err`. A lone `initialize` result is not this feature. The letters in the model text are not the pass criterion.
- **No sessionId.** Exit 3 means grok answered `session/new` without a `sessionId` (often auth). Save the transcript. That is not a pass.
- **Proof.** Save `probe.ndjson`, `probe.err`, `probe.cmd.txt`, and `doctor.json`. The entry point is 7678 with a bearer token.

## Gotchas

- Phone `initialize` is answered by topside. It works with no home agent. Do not treat it as `session/prompt`.
- `launch-home` starts its own `grok agent leader` on `run/$AQUALUNG_VERIFY_RUN/leader.sock`. That is not the interactive grok window unless you pass that window's socket with `--socket`. Cleanup kills only a leader this run started.
- `session/load` is a different feature. This command does not load.
- Timeout defaults to 240 seconds. Host-away retries for 120 seconds; a model error does not retry.
