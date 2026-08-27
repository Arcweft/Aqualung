---
name: recall
description: "Reconstruct recent working context from git, gh, optional chat history, and the shared record (user reports, prior fixes, incidents), then hand back a tight current-state brief. Use for 'recall my work on X', 'catch me up', 'what have I been working on', 'where did I leave off', before starting or resuming work."
disable-model-invocation: true
---

# Recall

**Before you start or resume work, you rebuild the recent working context and hand back a tight capsule of where things stand now and what to do next.** Use for "recall my work on X", "catch me up", "what have I been working on", or "where did I leave off".

Keep it tight and on-topic. Read only what the in-scope threads need, then stop. Heavy reading can fan out to workers. The main thread keeps only their findings and the final brief.

Workers follow [host.md](../host.md). Do not name a model.

Your context lives in two records. Live state is git and `gh`: branches, PRs, issues, the working tree. The shared record is everything that happened around the same code under other names: the symptoms users keep reporting, the fixes that shipped and got reverted. That second record is what the **why** skill searches. A feature with a long bug tail keeps most of its story there.

Chat history is optional. If the host exposes this session's or this workspace's chat logs, search them. If it does not, skip that slice and say so. Do not glob `~/.cursor/projects/*/`.

1. Classify, then route. If the user already gave a full state capsule (paths, branch, the change), use it and skip the mining. A human-readable summary of your work is a different task. Recall loads working context before you act.
2. Lock the scope before searching. Pin the window ("recent" is a real range, default the last 7 days), the topic if named, and this repository. State the scope back. Never quietly turn "all" into "recent N".
3. Sweep live state with `git` and `gh`: recent commits, open PRs, the current branch, uncommitted diffs. If chat logs exist, search them the same way: topic first, then only matching threads. Each slice returns the same schema, one block per thread: topic, the user's goal, decisions, open threads, struggles and corrections, and artifacts (PRs, tickets, branches). Cite PR numbers and commit hashes. Cite a chat id only when you actually read that chat.
4. Sweep the shared record whenever the topic names a feature, file, subsystem, area, or bug. This is the default, not a judgment call. Hand it to the **why** skill, but steer the question from "why was this built this way" to "what's the current state, what's been tried and didn't hold, and what are users still reporting". Inherit `why`'s posture: null results are findings, skip an unavailable source and say so. Skip this step only for pure activity recall with no named target ("what did I do this week").
5. Verify against live state. A stale ticket is history, not current truth, so take the PRs, branches, and tickets that the sweep surfaced and check them with `git` and `gh`.
6. Write the brief to the contract below. Group by thread. Stay on the named topic.

## Output contract

Lead with the capsule, then the thread status, then the problems, then the next move. Deeper detail goes below or gets cut.

- **Capsule.** At most 5 bullets. What this work is and where it stands overall.
- **Threads.** One line each, prefixed with exactly one status tag: `[merged #N]`, `[open PR #N]`, `[in flight <branch>]`, `[verified, uncommitted]`, `[reverted #N]`, or `[planned, not started]`. A thread with no tag is not done yet, so tag it.
- **Problems.** At most 5, the recurring ones. Include the symptoms users keep reporting and any fix that shipped and was reverted, so the next attempt starts where the last one failed.
- **Next move.** The single most useful next action, concrete.

An adjacent feature or ticket stays out unless it blocks this one. When the capsule and thread lines outgrow a screen, cut detail before you cut threads. Write the brief through the **unslop** skill, cite shared-record findings by their source (PR #, ticket ID, commit hash), and sanitize private context before any public output.

**Reply:** the brief, to the contract above.
