# aqualung verification map

This directory is the maintained source for verifying the user-facing behavior of aqualung. Read the index before driving, then use the matching feature file as the recipe.

## Baseline preconditions

- Export `AQUALUNG_VERIFY_RUN` and put `.agents/skills/verify-aqualung/bin` on `PATH`.
- Run `control-aqualung doctor --save`. Drive only when it exits 0.
- If doctor exits 2 (`design`) or 1 (`incomplete`), save `doctor.json` and stop. The features below are unreachable.
- Never drive a listener on 1943 or 7678 that this run did not start.
- topside serves one snorkel. Do not start a second verification run against the same ports.

## Driving conventions

- Start every recipe from the baseline unless its preconditions say otherwise.
- Treat every command as literal. Keep quoted JSON and flags unchanged.
- Phone actions go through `control-aqualung phone`.
- Restore nothing on disk for topside. It writes none. Scratch certs and tokens live only in this run's state.
- Do not remove proof artifacts during cleanup.

## Proof and skip reporting

- Capture the user action and the resulting state, not only the last frame.
- Phone proof includes the JSON-RPC request, the response body, and the doctor JSON.
- Mutation proof includes a second client or a reconnect, not the same socket reread.
- Record the feature ID and entry point with every artifact.
- Report an unreachable path with the doctor transcript and the unmet precondition.
- Do not report a skipped entry point as verified through a different path.

## Feature entry contract

Each feature file starts with an H1 title and one paragraph describing the user-visible behavior. It then uses exactly four H2 sections in this order.

1. `Sub-features` lists short IDs with one line for each behavior.
2. `How to get to it (user POV)` lists every user entry point.
3. `Driving it with control-aqualung` starts with `Preconditions:` and uses labeled bullets that pair each user action with an exact command and observable result.
4. `Gotchas` lists traps that can waste or invalidate a verification run.

Keep implementation details out of the map. Name only user paths, stable handles, required state, commands, and observable proof.

## Features

- [Phone attach](./phone-attach.md) covers bearer auth on 7678 and topside answering `initialize`.
- [Host away](./host-away.md) covers the phone being told the host is away when home is offline.
- [Snorkel replace](./snorkel-replace.md) covers a new snorkel connection replacing the wedged one.
- [Session fan-out](./session-fanout.md) covers session updates reaching every phone watching that session.
- [Home bypass](./home-bypass.md) covers the local editor talking to the home socket without topside.
