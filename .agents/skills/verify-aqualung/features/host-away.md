# Host away

Host away tells every connected phone that the machine at home is offline, instead of leaving the WebSocket idle or closing it without a reason.

## Sub-features

- `away-drop` surfaces host-away after snorkel or the home agent goes away.
- `away-stays-up` keeps the phone WebSocket up so the phone can wait for home.
- `away-clears` removes the away state when snorkel is back and authenticated.

## How to get to it (user POV)

- Stay connected on a phone while the home machine goes offline.
- Stay connected while snorkel dies and has not yet been replaced.

## Driving it with control-aqualung

Preconditions:

- Doctor exits 0. A phone is already attached per [phone-attach](./phone-attach.md).
- A snorkel owned by this run is the current home connection.

- **Watch.** Keep one phone socket open. There is no long-lived `phone` subcommand yet. Use `control-aqualung phone` only for single round trips. If Launch has not grown a listen mode, record that entry point as unreachable with the attempted command, and do not claim away was verified by reconnecting later.
- **Drop home.** Stop the snorkel PID recorded in `run/$AQUALUNG_VERIFY_RUN/state` only, not some other process named snorkel. The phone receives an ACP notification or JSON-RPC error that the host is away. The socket stays open.
- **Home back.** Launch snorkel again the way Launch documents. The away state clears on the same phone connection or the phone can `initialize` again without a new token dance beyond what Launch documents.
- **Proof.** Save the away payload and a post-reconnect `initialize` body under `artifacts/$AQUALUNG_VERIFY_RUN/host-away/`. Closing 7678 is not host-away.

## Gotchas

- TCP close on 7678 means topside died or refused the phone. That is not host-away.
- Killing an unowned snorkel on the machine is forbidden. If this run did not start it, stop.
- Reconnecting after a drop proves re-attach, not that the first connection was told away.
