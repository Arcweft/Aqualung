---
name: verify-aqualung
description: "Verify aqualung. Drive snorkel (home mTLS dialer on 1943) and topside (phone ACP WebSocket on 7678). Use when proving phone attach, session load, host-away, snorkel replace, session fan-out, local-socket bypass, or any change that could affect those paths."
---

# Verify aqualung

aqualung is two programs. `snorkel` runs next to the agent at home and dials out. `topside` runs on a server you control and speaks ACP to phones. This skill drives those processes the way a user does.

Both binaries exist. `control-aqualung doctor` after local Launch or `launch-image` exits 0 with `"stage": "ready"` when this run owns the instance. Drive only then.

Read `features/README.md` before driving. Drive every listed entry point for the feature under test. An unreachable path is reported with the doctor transcript, never as a pass through a different path.

Repo root is three levels above this file: the directory that contains `README.md`.

## Launch

Put the helper on `PATH` or invoke it by path:

```
.agents/skills/verify-aqualung/bin/control-aqualung
```

Set `AQUALUNG_VERIFY_RUN` to a unique id for this attempt. All artifacts and PID state key off it.

```
export AQUALUNG_VERIFY_RUN=verify-$RANDOM
export PATH="$PWD/.agents/skills/verify-aqualung/bin:$PATH"
```

Run `control-aqualung launch`. Completion: stdout is a doctor JSON object.

Launch mints a throwaway PKI and bearer token under `run/$AQUALUNG_VERIFY_RUN/`, starts both processes, and writes their PIDs to `run/$AQUALUNG_VERIFY_RUN/state`. The token is also written to `run/$AQUALUNG_VERIFY_RUN/token`.

```
target/debug/topside \
  --cert run/$AQUALUNG_VERIFY_RUN/pki/server.pem \
  --key run/$AQUALUNG_VERIFY_RUN/pki/server.key \
  --ca run/$AQUALUNG_VERIFY_RUN/pki/ca.pem \
  --client-cert run/$AQUALUNG_VERIFY_RUN/pki/client.pem \
  --token "$(cat run/$AQUALUNG_VERIFY_RUN/token)" \
  --snorkel 0.0.0.0:1943 \
  --phone 0.0.0.0:7678
```

```
target/debug/snorkel \
  --socket run/$AQUALUNG_VERIFY_RUN/missing.sock \
  --server 127.0.0.1:1943 \
  --cert run/$AQUALUNG_VERIFY_RUN/pki/client.pem \
  --key run/$AQUALUNG_VERIFY_RUN/pki/client.key \
  --ca run/$AQUALUNG_VERIFY_RUN/pki/ca.pem
```

The unix socket path does not exist. snorkel will not dial TLS. topside still owns 1943 and 7678, so doctor can be `ready` and phones can attach. This Launch does not hold a phone socket, so host-away, replace, and fan-out stay proven by `cargo test -p topside`, not by sequential `initialize`. `session/load` is unreachable on this Launch.

To prove Register and `session/load` against a real Grok `leader.sock`, start the published image instead. Docker random-maps 7678 and 1943 so a local Launch on those ports is a separate conflict, not this command:

```
control-aqualung launch-image
control-aqualung launch-image ghcr.io/arcweft/aqualung:alpha
```

Default image is `ghcr.io/arcweft/aqualung:stable`, or `AQUALUNG_VERIFY_IMAGE`. Completion: stdout is a doctor JSON object with `"launch": "docker"`. Launch waits until `/var/lib/grok/leader.sock` exists and grok has logged `leader.client.registered`. The token is still `run/$AQUALUNG_VERIFY_RUN/token`. Cleanup is `docker rm` of that container only.

`launch-image` sets `XAI_API_KEY` to `aqualung-smoke-not-a-key` unless the environment already has `XAI_API_KEY`, and mounts `docker/grok-smoke-auth.toml` so grok skips the live API-key probe. That is enough for `session/new` and `session/load`. It is not a login. `session/prompt` will fail with the dummy.

`launch-image` does not need host `snorkel`/`topside` binaries. Local `launch` still does.

Local `launch` exits:

- Exit 2 and `"stage": "design"`: neither binary exists. Stop. `launch-image` is still allowed.
- Exit 1 and `"stage": "incomplete"`: only `snorkel` or only `topside` is present. Stop. Do not invent the missing binary.
- Exit 1 and `"stage": "foreign"`: something this run did not start already owns 1943 or 7678. Stop. Do not kill it. `launch-image` binds random host ports and is not this case.
- Exit 1 and `"stage": "idle"`: binaries exist but this run has not launched, or an owned docker container is not running.

Ready means this run owns the instance and doctor exits 0. For local Launch, both binaries exist and this run owns 1943 and 7678. For `launch-image`, the named container is running. snorkel dials `1943/tcp` with mutual TLS (one client certificate). Phones connect on 7678 inside the container, or on the `phone_port` in doctor JSON, with a bearer token, ACP over WebSocket, one JSON object per message.

topside serves one snorkel. A new authenticated snorkel replaces the old one. Two verification runs on the same ports will steal each other. One run per machine unless you rewrite Launch with distinct ports from real flags.

Teardown is `control-aqualung cleanup` with the same `AQUALUNG_VERIFY_RUN`.

## Doctor

Whenever anything looks off, run this first:

```
control-aqualung doctor --save
```

Completion: stdout JSON, exit code as below, and with `--save` a copy at `.agents/skills/verify-aqualung/artifacts/$AQUALUNG_VERIFY_RUN/doctor.json`.

| Exit | `stage` | Meaning |
|------|---------|---------|
| 0 | `ready` | This run owns a driveable instance, local or docker. |
| 2 | `design` | No `snorkel` or `topside` on `PATH` or in `target/{release,debug}`, and this run has no docker container. |
| 1 | `foreign` | Port 1943 or 7678 is listening and `run/$AQUALUNG_VERIFY_RUN/state` is missing. |
| 1 | `incomplete` / `idle` | Half a binary pair, binaries exist but this run never launched, or the owned container is not running. |

Drive only on exit 0. On exit 2, save the JSON and stop; the mapped features are unreachable.

## Drive

Commands go through `control-aqualung`. Feature files under `features/` name the user path, the exact command, and the observable result.

Phone path, only after doctor exits 0:

```
control-aqualung phone --token "$TOKEN" --send '{"jsonrpc":"2.0","id":0,"method":"initialize","params":{}}'
```

That opens `ws://127.0.0.1:$port/`, sends `Authorization: Bearer $TOKEN`, writes each `--send` as its own text frame, and prints one JSON-RPC reply per request, matched by `id`. Notifications such as `aqualung/host_away` go to stderr. topside answers `initialize` and authentication itself. Assert a JSON-RPC body (result or error) from that process, not a TCP drop and not a reply that could only have come from the home agent.

For `session/new` then `session/load` on one connection after `launch-image`:

```
control-aqualung session-load
```

Completion: exit 0 and `artifacts/$AQUALUNG_VERIFY_RUN/session-load/probe.ndjson` containing a `sessionId` plus a `session/load` reply. Exit 3 means grok answered `session/new` without a `sessionId`; that is not a pass. The command retries while the error is `host is away`.

Snorkel path is mTLS to `1943/tcp`. Local Launch starts snorkel with `--socket` pointing at a missing unix path, so it does not dial TLS. Do not invent a live home agent on that path. `launch-image` is the live socket.

Stable handles: port `1943`, port `7678` or the `phone_port` in doctor JSON, header `Authorization: Bearer`, JSON-RPC methods `"initialize"`, `"session/new"`, `"session/load"`, one JSON object per WebSocket message. The README says the interface is not stable; assert behavior the README already states, not field names from a future binary.

`control-aqualung phone` refuses unless doctor is driveable. Do not bypass it with a raw socket to a listener you do not own.

## Evidence

Write under `.agents/skills/verify-aqualung/artifacts/$AQUALUNG_VERIFY_RUN/`. Cleanup must not delete this directory.

Every proof includes:

- The doctor JSON from that run (`doctor.json`).
- The command, stdout, stderr, and exit code for each drive step (`*.cmd.txt` next to the response body).
- The feature id and entry point in those files.

Proof standards:

- Exercise the user path. Phones go through 7678 with a bearer token. Snorkel goes through 1943 with the client cert. Local editors go through the home unix socket and must not be satisfied by probing topside.
- Capture the action and the resulting state. A successful `initialize` is not proof of session fan-out. A closed 7678 is not proof of host-away; host-away is a message to an already-connected phone after the home side drops.
- Side effects: topside writes nothing to disk. After restart, phones re-attach by loading the session again. Prove that by reconnecting, not by looking for files under topside.
- Mocks only at a production boundary that already isolates an external system. A fake topside is not aqualung. Do not stand one up to make a feature look green.

On a checkout with both binaries, Launch starts them, doctor `--save` exits 0 with `"stage": "ready"`, and `control-aqualung phone` with the Launch token proves phone-attach. Host-away, snorkel-replace, and session-fan-out are crate tests (`cargo test -p topside`) until Launch grows a listen mode that holds a phone socket. Sequential `initialize` is not those features. Home-bypass stays unreachable while local Launch points snorkel at a missing unix socket.

`session/load` is proven with `launch-image` then `session-load`, not with local Launch and not with `cargo test`. A successful `initialize` is not that proof. `docker/smoke.sh` runs the same `session/new` + `session/load` probe against a GHCR tag before moving `:stable` / `:alpha`.

## Cleanup

```
control-aqualung cleanup
```

Completion: the PID file `run/$AQUALUNG_VERIFY_RUN/state` is gone, and any PIDs listed there are not running. Artifacts remain.

Kill only those recorded PIDs. Never `pkill snorkel` or `pkill topside`. If there is no state file, cleanup prints `no owned instance` and exits 0.

After a failed attempt, run cleanup with the same `AQUALUNG_VERIFY_RUN` before starting another.

## Helpers

`bin/control-aqualung` is executable. Phone frames are sent by `docker/phone.py`. `bin/phone.py` runs that file. Invocations:

```
control-aqualung doctor
control-aqualung doctor --save
control-aqualung launch
control-aqualung launch-image
control-aqualung cleanup
control-aqualung phone --token <token> --send <json>
control-aqualung phone --token <token> --send <json> --send <json>
control-aqualung session-load
```
