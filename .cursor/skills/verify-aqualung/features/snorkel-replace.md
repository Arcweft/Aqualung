# Snorkel replace

Snorkel replace lets a newly authenticated home connection take over so a wedged snorkel at home cannot lock phones out.

## Sub-features

- `replace-second` accepts a second authenticated snorkel on 1943.
- `replace-drops-first` ends the first snorkel's session.
- `replace-phones-live` leaves already-attached phones able to talk to the new home connection.

## How to get to it (user POV)

- Start a second snorkel from home while the first is still connected.
- Restart snorkel after it wedges, using the same client certificate.

## Driving it with control-aqualung

Preconditions:

- Doctor exits 0.
- This run owns the current snorkel PID in `run/$AQUALUNG_VERIFY_RUN/state`.
- A second snorkel can be started with the same client cert the server trusts. Launch must document that command. If it does not, this feature is unreachable.

- **First link.** Confirm the owned snorkel is the live home side. A phone `initialize` already succeeded.
- **Second link.** Start a second snorkel the way Launch documents. The second mTLS handshake on 1943 succeeds.
- **First is gone.** The first snorkel is no longer the live home side. Traffic from it does not reach phones. Do not prove this by `pkill`.
- **Phones still attached.** Run `control-aqualung phone --token "$TOKEN" --send '{"jsonrpc":"2.0","id":0,"method":"initialize","params":{}}'` again, or observe the existing phone socket. Phones are not locked out.
- **Proof.** Save both snorkel start transcripts and the post-replace phone body under `artifacts/$AQUALUNG_VERIFY_RUN/snorkel-replace/`.

## Gotchas

- topside trusts exactly one client certificate. A second cert is a different user, not a replace.
- Two verification runs on 1943 will replace each other. That is a contaminated run, not this feature.
- Replacing snorkel is not host-away. Host-away is what phones see when no snorkel is live.
