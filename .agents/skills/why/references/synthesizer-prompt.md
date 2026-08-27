# Synthesizer Prompt Template

Build the synthesizer pass from this template. Fill in the placeholders.

---

You are answering a "why" question about a piece of code by synthesizing findings from historical sources (git and `gh` at minimum; other sources only if they were actually searched). Produce a confidence-weighted, evidence-cited narrative that honestly communicates what the evidence supports and what it doesn't.

## The Question

> {QUESTION}

## The Code Anchor

**Target files:** {FILES_WITH_LINE_RANGES}

**Key symbols:** {SYMBOLS}

## Investigator Findings

{ALL_INVESTIGATOR_FINDINGS}

## Sources That Weren't Searched

{SKIPPED_SOURCES_WITH_REASONS}

## Epistemics Framework

You MUST follow the framework in `references/epistemics.md`. Read it in full before writing the output. The key rules:

1. Every claim sits in one of these tiers: **Direct**, **Supported**, **Inferred**, **Speculative**, **Unknown**. The tier determines what section the claim goes in and how it's phrased.
2. Every Direct/Supported claim must have a citation (PR #, ticket ID, doc URL, chat permalink, commit hash, or file:line).
3. Inferred and Speculative claims must use hedged language ("appears to", "likely", "suggests", "one possibility is").
4. Never cite code as evidence for its own intent.
5. Gaps in the evidence must be documented. Don't fill them with plausible-sounding guesses.
6. If the user's question embedded a hypothesis, treat it as a candidate, not a conclusion. Check the evidence independently.

## Instructions

1. **Read all findings.** They gathered raw evidence, not conclusions. You weigh it.
2. **Reconcile overlapping findings.** Merge duplicate citations into a single, authoritative reference.
3. **Identify contradictions.** If two items of evidence disagree, don't pick one. Surface both.
4. **Calibrate confidence.** For each claim, identify the evidence and the tier.
5. **Verify citations by spot-checking.** Read the repo and the tools you have. Do not write files, commit, or modify external state. If you're uncertain a cited item exists or says what's claimed, check it.
6. **Don't overreach.** Better to leave an open question open than to fill it with a confident-sounding guess.

## Output Format

Write the output for the user. Use this exact structure:

---

### The Question

Restate the user's question in one or two sentences so the answer is anchored.

### The Code in Question

File paths, line ranges, key symbols. Two or three lines to orient a reader who lands here cold.

### What We Found

**Claims with direct evidence**, one per bullet. Quote or paraphrase the source and cite precisely. Format each finding like:

- **[Direct]** {Claim}. Source: [PR #123](url) / ticket ID / file:line. {Brief quote or paraphrase.}
- **[Supported]** {Claim}. Evidence: {list of items and what each contributes}.

Use `[Direct]` for single-source, explicit evidence. Use `[Supported]` when multiple indirect items converge on a conclusion.

### What We Can Reasonably Infer

**Claims that aren't explicitly stated anywhere but are well-supported by indirect evidence.** Make the inference chain visible: "Given A and B, it's likely that C." Use hedged language ("appears to", "likely", "suggests", "is consistent with"). Format:

- **[Inferred]** {Hedged claim}. Reasoning: {the specific evidence and the inference step}.

If there's nothing to infer, skip this section.

### Competing Hypotheses

**If the evidence fits multiple stories, present them.** Don't force a winner when the record doesn't support one. For each hypothesis:

- **Hypothesis:** {one-sentence statement}
- **Evidence for:** {specific items}
- **Evidence against or missing:** {what would need to be true but isn't, or what counter-signals exist}

Skip this section if there's a single clear answer.

### What We Don't Know

**Explicit gaps.** Things the user asked that the evidence didn't answer. Sources searched that came up empty. Sources that weren't searchable.

Be specific. Include:

- Specific questions that went unanswered
- Searches that returned nothing
- Sources that were unavailable (and why)
- People who would likely know but who you can't ask

### Sources Consulted

Bulleted list of what was actually searched. Always include source control. Extra sources appear only if searched or explicitly skipped.

- **Source control history**: {file paths}, {number of commits reviewed}, PRs #{numbers}, and code comments searched. Git and `gh` are the floor; "not searched" here is a failure of the run.
- **Issues**: {ids and keyword searches}, or skipped with reason.
- **Long-form documents / chat / observability / errors / analytics**: searched queries, or "skipped. no matching tools."

### Confidence Summary

One or two sentences summarizing overall confidence. Name what is direct, what is inferred, and what could not be answered.

---

## Quality Check Before Returning

1. Does every claim in "What We Found" have a citation? If not, add one or move the claim to "Inferred" or "Hypotheses."
2. Is the phrasing tier-appropriate? (Direct claims can use "because"; Inferred claims cannot.)
3. Did you surface any contradictions you noticed, or did you quietly pick one?
4. Does the "What We Don't Know" section exist and name specific gaps?
5. If the user embedded a hypothesis in their question, did you check it against the evidence rather than rubber-stamping it?
6. Did you cite any code as evidence for its own intent? Remove those.
7. Is the overall tone calibrated? A confident-sounding answer with weak evidence is the failure mode this skill exists to prevent.

If any item fails, revise before returning.

The value of this output comes from its honesty, not its authority. Be clear about what's known, what's inferred, and what's missing.
