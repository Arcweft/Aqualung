# Home bypass

Home bypass is the local editor or terminal talking to the agent unix socket on the home machine. That path must not go through topside. If topside dies, work at home is unaffected.

## Sub-features

- `bypass-socket` the home editor uses the local unix socket, not 7678.
- `bypass-topside-down` stopping topside does not break the home editor session.
- `bypass-no-phone-cap` topside never registers tool, filesystem, or terminal capabilities on behalf of a phone.

## How to get to it (user POV)

- Use the editor or terminal already attached at home.
- Kill or restart the server running topside while continuing to work at home.

## Driving it with control-aqualung

Preconditions:

- Doctor exits 0 for the topside/snorkel pair if you also need phones up as a contrast.
- The home agent socket path is the one Launch documents. If Launch has no socket path, this feature is unreachable. Do not guess `~/.cursor/` or `/tmp/`.

- **Home path.** Send a trivial ACP `initialize` (or the agent's documented hello) on the unix socket Launch named, not via `control-aqualung phone`. The agent answers. That command is the editor entry point.
- **Topside down.** Kill only the topside PID in `run/$AQUALUNG_VERIFY_RUN/state`. Repeat the unix-socket hello. It still succeeds. `control-aqualung phone` now fails to connect on 7678.
- **No borrowed caps.** From a phone, after attach, the advertised capabilities must not include tools, filesystem, or terminals that only exist at home unless the home agent itself advertised them. topside must not add them.
- **Proof.** Save the unix-socket transcript, the post-kill unix-socket transcript, and the failed phone connect under `artifacts/$AQUALUNG_VERIFY_RUN/home-bypass/`. A green phone path is not this feature.

## Gotchas

- Probing 7678 after killing topside only proves topside died. The feature also needs the home socket still working.
- snorkel copies bytes and does not speak ACP. Do not treat a snorkel log line as the editor session.
- The README names this as a non-goal for topside: it does not run tools, read files, or open terminals.
