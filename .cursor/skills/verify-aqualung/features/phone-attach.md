# Phone attach

Phone attach lets a phone open ACP on topside with a bearer token and get `initialize` and authentication from topside itself, without the home agent having to speak those methods.

## Sub-features

- `phone-connect` upgrades to WebSocket on 7678 with a bearer token.
- `phone-reject` refuses a missing or wrong token before ACP starts.
- `phone-initialize` returns a JSON-RPC `initialize` result or error from topside.

## How to get to it (user POV)

- Connect a phone client to topside on 7678/tcp with the bearer token and send ACP `initialize`.

## Driving it with control-aqualung

Preconditions:

- `control-aqualung doctor --save` has been run for this `AQUALUNG_VERIFY_RUN`.
- Doctor exits 0. If it exits 2, write `doctor.json` and stop. This feature is unreachable.

- **Rejected token.** Connect with a bad token. Run `control-aqualung phone --token "wrong" --send '{"jsonrpc":"2.0","id":0,"method":"initialize","params":{}}'`. Exit code is non-zero. stderr shows the handshake or TCP failed. No JSON-RPC result is printed.
- **Accepted token.** Connect with the token this instance was launched with. Run `control-aqualung phone --token "$TOKEN" --send '{"jsonrpc":"2.0","id":0,"method":"initialize","params":{}}'`. Exit code 0. stdout is one JSON-RPC object, a result or error for id `0`.
- **Answered here.** The `initialize` body is from topside. It must not require a live home agent, and it must not be an empty TCP close.
- **Proof.** Save stdout to `artifacts/$AQUALUNG_VERIFY_RUN/phone-attach/initialize.json` and the doctor JSON beside it. Both files name this feature and the 7678 entry point.

## Gotchas

- Doctor exit 2 is an unmet precondition, not a failed attach.
- A listener on 7678 that this run did not start is foreign. Do not send it a token.
- topside answers `initialize` and authentication. A timeout waiting for the home agent is a fail.
- One JSON-RPC object per WebSocket message. Concatenated objects in one frame are not this protocol.
