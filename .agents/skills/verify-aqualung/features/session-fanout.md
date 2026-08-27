# Session fan-out

Session fan-out delivers the same session updates to every phone that is watching that session, over the single snorkel connection.

## Sub-features

- `fanout-two-phones` sends one session update to two attached phones.
- `fanout-ids` rewrites JSON-RPC request ids so two phones do not collide on the one home connection.
- `fanout-unrelated` does not deliver a session's updates to a phone that is not watching it.

## How to get to it (user POV)

- Open the same session on two phones.
- Prompt from one phone and watch the other.

## Driving it with control-aqualung

Preconditions:

- Doctor exits 0.
- Phone attach works for two tokens or two connections with the same token, whichever Launch documents as the real phone auth.
- `control-aqualung phone` is a single round trip. If Launch has not grown two concurrent listeners, record fan-out as unreachable. Do not fake it with two sequential `initialize` calls.

- **Attach A.** Connect phone A and `initialize`.
- **Attach B.** Connect phone B and `initialize` while A stays up.
- **Watch the same session.** Put both on one session the way ACP and Launch document (`session/new` or `session/resume` once those flags exist in this repo).
- **Prompt from A.** Send a prompt from A. B receives the matching `session/update` (or the repo's named equivalent) without B sending the prompt.
- **Proof.** Save both phones' transcripts under `artifacts/$AQUALUNG_VERIFY_RUN/session-fanout/`. Sequential round trips on one socket are not fan-out.

## Gotchas

- topside multiplexes several phones onto one snorkel. Proving one phone works does not prove fan-out.
- Id rewriting is user-visible when two phones issue requests at once. If ids collide and one phone sees the other's result, the feature failed.
- A second phone that only gets `initialize` has not demonstrated session fan-out.
