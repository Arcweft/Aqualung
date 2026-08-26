---
name: verify-aqualung
description: "Verify aqualung. Drive snorkel (home mTLS dialer on 1943) and topside (phone ACP WebSocket on 7678). Use when proving phone attach, host-away, snorkel replace, session fan-out, local-socket bypass, or any change that could affect those paths."
---

# Verify aqualung

aqualung is two programs. `snorkel` runs next to the agent at home and dials out. `topside` runs on a server you control and speaks ACP to phones. This skill drives those processes the way a user does.

This checkout is design-stage. `README.md` Status says there is no code yet. `control-aqualung doctor` must exit 2 until `snorkel` and `topside` binaries exist. That exit is the proof the tree matches the README. It is not a skip of the mapped features.

Read `features/README.md` before driving. Drive every listed entry point for the feature under test. An unreachable path is reported with the doctor transcript, never as a pass through a different path.

Repo root is three levels above this file: the directory that contains `README.md`.

## Launch

Put the helper on `PATH` or invoke it by path:

```
.cursor/skills/verify-aqualung/bin/control-aqualung
```

Set `AQUALUNG_VERIFY_RUN` to a unique id for this attempt. All artifacts and PID state key off it.

```
export AQUALUNG_VERIFY_RUN=verify-$RANDOM
export PATH="$PWD/.cursor/skills/verify-aqualung/bin:$PATH"
```

Run `control-aqualung launch`. Completion: stdout is a doctor JSON object.

- Exit 2 and `"stage": "design"`: no binary to start. Stop. Do not invent `cargo run`, `npm start`, or listen ports yourself.
- Exit 1 and `"stage": "foreign"`: something this run did not start already owns 1943 or 7678. Stop. Do not kill it.
- Exit 1 after binaries appear: the README still has no start command. Read `snorkel --help` and `topside --help`, then rewrite this section with `/maintain-verification-skill` before launching.

Ready means both binaries exist, this run owns the listeners, and doctor exits 0. Documented defaults from the README, not from guesswork: snorkel dials `1943/tcp` with mutual TLS (one client certificate). Phones connect on `7678/tcp` with a bearer token, ACP over WebSocket, one JSON-RPC object per message.

topside serves one snorkel. A new authenticated snorkel replaces the old one. Two verification runs on the same ports will steal each other. One run per machine unless you have rewritten Launch with distinct ports from real flags.

Teardown is `control-aqualung cleanup` with the same `AQUALUNG_VERIFY_RUN`.

## Doctor

Whenever anything looks off, run this first:

```
control-aqualung doctor --save
```

Completion: stdout JSON, exit code as below, and with `--save` a copy at `.cursor/skills/verify-aqualung/artifacts/$AQUALUNG_VERIFY_RUN/doctor.json`.

| Exit | `stage` | Meaning |
|------|---------|---------|
| 0 | `ready` | This run owns a driveable instance. |
| 2 | `design` | No `snorkel` or `topside` on `PATH` or in `target/{release,debug}`. |
| 1 | `foreign` | Port 1943 or 7678 is listening and `run/$AQUALUNG_VERIFY_RUN/state` is missing. |
| 1 | `incomplete` / `idle` | Half a binary pair, or binaries exist but this run never launched. |

Drive only on exit 0. On exit 2, save the JSON and stop; the mapped features are unreachable.

## Drive

Commands go through `control-aqualung`. Feature files under `features/` name the user path, the exact command, and the observable result.

Phone path, only after doctor exits 0:

```
control-aqualung phone --token "$TOKEN" --send '{"jsonrpc":"2.0","id":0,"method":"initialize","params":{}}'
```

That opens `ws://127.0.0.1:7678/`, sends `Authorization: Bearer $TOKEN`, writes one text frame, prints one text frame. topside answers `initialize` and authentication itself. Assert a JSON-RPC body (result or error) from that process, not a TCP drop and not a reply that could only have come from the home agent.

Snorkel path is mTLS to `1943/tcp`. There is no probe subcommand until Launch documents how snorkel is started and where it takes the unix socket and client cert. Do not invent those flags.

Stable handles: port `1943`, port `7678`, header `Authorization: Bearer`, JSON-RPC `method` `"initialize"`, one JSON object per WebSocket message. The README says the interface is not stable; assert behavior the README already states, not field names from a future binary.

`control-aqualung phone` refuses unless doctor is driveable. Do not bypass it with a raw socket to a listener you do not own.

## Evidence

Write under `.cursor/skills/verify-aqualung/artifacts/$AQUALUNG_VERIFY_RUN/`. Cleanup must not delete this directory.

Every proof includes:

- The doctor JSON from that run (`doctor.json`).
- The command, stdout, stderr, and exit code for each drive step (`*.cmd.txt` next to the response body).
- The feature id and entry point in those files.

Proof standards:

- Exercise the user path. Phones go through 7678 with a bearer token. Snorkel goes through 1943 with the client cert. Local editors go through the home unix socket and must not be satisfied by probing topside.
- Capture the action and the resulting state. A successful `initialize` is not proof of session fan-out. A closed 7678 is not proof of host-away; host-away is a message to an already-connected phone after the home side drops.
- Side effects: topside writes nothing to disk. After restart, phones re-attach by loading the session again. Prove that by reconnecting, not by looking for files under topside.
- Mocks only at a production boundary that already isolates an external system. A fake topside is not aqualung. Do not stand one up to make a feature look green.

On design-stage, the only valid proof is `doctor --save` with exit 2 and `"stage": "design"`. Do not call that a verification of phone attach.

## Cleanup

```
control-aqualung cleanup
```

Completion: the PID file `run/$AQUALUNG_VERIFY_RUN/state` is gone, and any PIDs listed there are not running. Artifacts remain.

Kill only those recorded PIDs. Never `pkill snorkel` or `pkill topside`. If there is no state file, cleanup prints `no owned instance` and exits 0.

After a failed attempt, run cleanup with the same `AQUALUNG_VERIFY_RUN` before starting another.

## Helpers

`bin/control-aqualung` is executable. `bin/phone.py` is the WebSocket client it calls. Invocations:

```
control-aqualung doctor
control-aqualung doctor --save
control-aqualung launch
control-aqualung cleanup
control-aqualung phone --token <token> --send <json>
```
