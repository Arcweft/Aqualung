# Incident & Postmortem Context

Not a separate source. A **cross-cutting angle**. Incidents often motivate defensive code ("we added this check after the X outage"), so if the target looks defensive (null checks, retry logic, timeout handling, rate limiting, feature flags, protocol guards), hunt for incident history in the sources you can actually search.

Inside git and `gh`:

- Commit messages like "fix for incident", "add defensive check", "revert" followed by "re-apply with..."
- PR bodies and review threads that name an outage, a timeout, a dropped connection, or a protocol mismatch
- Tests added in the same change whose names encode the failure (`host_away`, `replace`, `unauthorized`)

If the host also has docs, chat, or error tools, search those for the same window. Do not invent a Datadog or Slack search when those tools are absent. Record that gap.

If you find an incident link, fetch it. Postmortems typically have an "Action Items" section that ties directly to code changes. When a commit, a PR, and a test name corroborate, the evidence is stronger.

Skip this angle for code that does not look defensive.
