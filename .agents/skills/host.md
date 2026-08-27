# Host

Skills in this directory run on whichever agent loaded them. They do not require Cursor, Graphite, or a model config file.

- Do not name a model slug. If you spawn workers, the host picks the model.
- If the host can spawn workers, use them for the parallel steps in a skill. If it cannot, run those same steps inline or one after another. A missing Task tool is not a failure.
- Do not block on Graphite, `/loop`, `~/.cursor/rules`, or `control-ui` / `control-cli`.
- In this repository, drive snorkel and topside with `verify-aqualung`. Prove behavior on 1943 and 7678. Do not stand up a substitute server.
