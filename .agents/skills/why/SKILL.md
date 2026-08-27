---
name: why
description: "Use for 'why does X work this way', 'why we picked Y', design rationale, regressions, postmortems, or data-backed thresholds. Anchors in git and gh, names gaps for sources you cannot search, then returns a cited read on decisions and tradeoffs. Use how for runtime behavior."
---

# Why

Investigate the motivation and intent behind code. Why was it built this way? What edge cases were considered? What product, business, or operational constraints shaped the design? What alternatives were rejected, and why?

Companion to the `how` skill. `how` answers what the code does and how it works. `why` answers what forces led to its shape.

Workers follow [host.md](../host.md). Do not name a model.

## How this skill works

Motivation lives in the historical record, not in the current source. Start with git and `gh` (commits, PRs, review threads, linked issues, in-repo docs and tests). If the host also exposes other evidence (docs, chat, observability, error tracking, analytics), search those too. If it does not, record the gap. Do not invent a search you cannot run.

Null results are first-class evidence. "We searched PRs #12 and #18 and no review discussed this threshold" is an answer.

## Operating Posture

Operate as a careful, cautious, precise investigator. Think like a detective piecing together a historical case from fragmentary records. When the record is thin, say so.

Concretely:

- **Evidence before narrative.** Collect the pieces first, then see what story they support. Never pick a story and recruit the evidence that fits it.
- **Precision over polish.** Prefer the exact quote and citation over a smooth paraphrase. A reader should be able to follow any claim back to its source and verify it in under a minute.
- **Consider what you haven't seen.** The evidence you find is a sample, not the whole truth. Before concluding, ask what you would expect to see if an alternative explanation were true, and whether you looked for it.
- **Name the gaps.** If a thread goes cold, a source isn't searchable, or a question has no answer, document the gap. Don't paper it over with an authoritative-sounding guess.
- **Hedge on purpose.** When evidence is indirect, your language should signal it ("appears to", "likely", "suggests"). Confidence-matching phrasing is a feature of the output, not a stylistic choice the synthesizer may override.
- **No shortcut by code-reading.** The code tells you what it does, rarely why it exists. Resist inferring intent from code shape.

This posture is the working method, not a disclaimer.

## Core Epistemics

This skill builds a **patchwork understanding** from fragmented historical evidence. Tickets go stale. Chat threads get deleted. Commit messages lie. People change their minds between the PR description and the implementation. The original author may have left the company.

Be ruthlessly honest about what you know versus what you're inferring. The goal is not a satisfying story; it is to surface evidence, calibrate confidence, and let the user decide.

Principles:

- **Cite everything.** Every claim about intent should reference a specific commit hash, PR number, ticket ID, doc URL, chat permalink, or code comment. If you can't cite it, it's inference, not fact, and must be labeled as such.
- **Prefer "appears to" over "because".** Hedge when evidence is indirect. Reserve confident language for direct, explicit evidence.
- **Surface contradictions.** If two sources disagree, show both. Don't quietly pick the one that fits your narrative.
- **Acknowledge gaps.** If a question has no answer in any source you searched, say so. An honest "we couldn't find out why" beats a confident guess.
- **Multiple hypotheses are valid.** When the evidence fits several stories, present them all with the evidence for each. Let the user triangulate.
- **Beware rationalization.** Code that makes sense today may have been written for reasons that no longer apply, or for no good reason at all. Don't retrofit intent.

Read `references/epistemics.md` for the full confidence framework and phrasing guide. The synthesizer must follow it.

## Step 1. Understand the Target and the Question

Parse what the user is asking. The **target** is usually a chunk of code, a pattern, a feature, or a named design decision. The **question** is usually one of:

- "Why was X designed this way?" Design rationale.
- "Why do we do X instead of Y?" Tradeoff or alternatives.
- "What edge cases motivated this?" Defensive reasoning.
- "What business or product constraint led to this?" External forcing function.
- "Why does this code still exist?" Dead-code territory.
- "What's the history of X?" Broad archaeological sweep.

If the target is vague ("why do we do it this way?" with no clear referent), make your best guess from conversation context (open files, recent edits, what was just discussed). State your interpretation briefly so the user can redirect if you're off, then proceed.

## Step 2. Establish the Code Anchor

Before fanning out, anchor the investigation in concrete code. You need:

- The relevant file path(s) and line range(s)
- The key symbols (function names, type names, constants)
- An initial commit list. The last few commits touching the target.
- PR numbers from merge commits (pattern `(#1234)` in the subject line)

Build this inline. It's cheap, and every later pass needs it.

```bash
git blame -L <start>,<end> <file>
git log --follow -p -- <file>
git log --oneline -20 -- <file>
git log -1 --format=%B <commit>
```

Pull PR bodies and discussion via `gh` for any substantive commits:

```bash
gh pr view <number> --json title,body,author,createdAt,mergedAt,labels,closingIssuesReferences,comments,reviews
```

If `gh issue` or linked GitHub Issues are reachable, pull those too. Capture seed context (file paths, symbols, commits, PR numbers, linked ticket IDs) so later passes don't rediscover it.

## Step 3. Search the Record

**Always search source control.** Use `references/sources/code-archaeology.md`. Git and `gh` are the floor.

If the host can spawn workers, one worker can own source control while others own extra sources. If it cannot, do the searches yourself in sequence. Each search uses `references/investigator-prompt.md`.

Optional extra sources, only when the host actually has tools for them:

1. Issue / ticket tracker beyond what `gh` already pulled
2. Long-form documents
3. Real-time team chat
4. Infrastructure observability
5. Error / exception tracking
6. Product analytics

For each extra source you cannot search, write one gap line. Do not skip a source because you doubt it will have anything. Skip only when there is no tool, or the source is provably irrelevant (a build-time script has no runtime error tracker). "Probably not in chat" is not a skip.

If the target looks defensive (null checks, retry, timeout, rate limit, feature flags, protocol guards), also run `references/sources/incident-postmortem.md` inside the sources you can search.

If a single-commit target already has the complete answer in the PR body, you may answer inline only after confirming further searches would be redundant. Say so. This should be rare.

## Step 4. Synthesize

Combine the findings into the output format below. A synthesizer worker is optional. Use `references/synthesizer-prompt.md` and `references/epistemics.md`. Spot-check citations. Do not rewrite hedges to sound more sure.

## Step 5. Present

Present the synthesizer output. You may lightly edit for clarity. **Do not rewrite the confidence language.** Dropping the hedges to sound more authoritative is the failure mode this skill exists to prevent.

## Output Format

Adapt as needed, but keep the confidence separation intact.

**The Question**. Restate what the user asked, concisely.

**The Code in Question**. File paths, line ranges, and key symbols. One or two lines so the reader is anchored.

**What We Found (direct evidence)**. Claims with explicit citations (PR #, ticket ID, doc URL, chat permalink, commit hash, code comment with file:line). Each bullet is a thing we have textual evidence for. Use present tense and quote or paraphrase the source.

**What We Can Reasonably Infer**. Claims well-supported by indirect evidence or combinations of signals, but not explicitly stated anywhere. Each bullet must explain the inference chain: "Given A and B, it's likely that C." Use hedged language ("appears to", "likely", "suggests").

**Competing Hypotheses**. If the evidence fits multiple stories, list them. For each, give the hypothesis, the evidence for it, and the evidence against it. Don't force a winner when the record doesn't support one. (Skip this section if there's a clear answer.)

**What We Don't Know**. Explicit gaps. Questions the user asked that the evidence didn't answer. Sources we searched and came up empty. Be specific.

**Sources Consulted**. One line per source, including the ones that returned nothing and the ones skipped. Format: `- <Source>: <what was searched>. <what was found, or "no relevant results," or "skipped. reason">.`

Example:

- Source control (git/gh): `git log --follow crates/topside/src/lib.rs`, PRs #5, #6. Found PR #5 introduced the phone listener and linked the design doc.
- GitHub Issues: searched for "7678" and "initialize". No relevant results.
- Long-form docs: `docs/topside-inputs.md`. Found the handshake contract.
- Real-time team chat: skipped. No chat tools in this environment. Gap: conversational record not searched.
- Observability / errors / analytics: skipped. No matching tools.

After Sources Consulted, if the user's `why` question is a precursor to changing this code, convert the lineage into a Preserve / Change / Avoid / Risk constraint set.

## Common Failure Modes to Avoid

- **Confident storytelling**. A plausible narrative built from thin evidence. A bullet with no citation goes in "inferred" or "hypotheses," not "what we found."
- **Citing the code as evidence for its own intent**. "Handles the null case because it checks for null" is mechanics, not motivation.
- **Recency bias**. Assuming the most recent commit is authoritative. Trace back.
- **Sycophantic agreement**. If the user suggests a reason, treat it as a hypothesis and check the evidence.
- **Skipping the gaps section**. An honest accounting of what you couldn't find is part of the value.
- **Skipping git/`gh`.** Those are the floor. Everything else is extra.

## Reference Files

- `references/epistemics.md`. Confidence tiers and phrasing guide.
- `references/investigator-prompt.md`. Base prompt for a source search.
- `references/synthesizer-prompt.md`. Prompt for the final writeup.
- `references/sources/code-archaeology.md`. git and `gh`.
- `references/sources/incident-postmortem.md`. Extra queries when the code looks defensive.
