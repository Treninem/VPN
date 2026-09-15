# AMRI repository working protocol

This file is a persistent handoff contract for every developer, coding agent, chat, or account that works on this repository.

## Before changing code

1. Read `docs/DEV_JOURNAL.md` first.
2. Read only the architecture documents relevant to the current stage; do not repeat a full audit unless the journal says it is necessary.
3. Check current `main`, recent commits, open pull requests and active work branches before starting a new branch. Preserve concurrent work instead of overwriting it.
4. Treat `docs/DEV_JOURNAL.md` as the canonical compact project memory when chat/account memory is unavailable.

## While working

- Work in coherent stages instead of many unrelated micro-commits.
- Keep security boundaries fail-closed.
- Never place credentials, subscription URLs, raw node URIs, user browsing history or personal identifiers in logs, debug output, process arguments, telemetry or the development journal.
- Preserve approved brand assets and unrelated concurrent changes.
- Do not claim VPN protection until transport, packet forwarding and the relevant leak protection are actually confirmed.

## Mandatory journal update after every substantial stage

Update `docs/DEV_JOURNAL.md` before considering a stage complete. Record concise engineering memory, not a transcript of the chat. Include:

- **What changed** — components/files and externally meaningful behavior.
- **Why** — the technical reason for the decision.
- **Alternatives/trade-offs** — important rejected approaches and why they were rejected.
- **Verification** — tests/CI/integration checks and their result.
- **Current state** — what is genuinely working now.
- **Next priorities** — the next few high-value steps in order.
- **Known issues/risks** — unfinished or deliberately disabled behavior.
- **Architecture decisions** — decisions another developer must not accidentally undo.

Do **not** record private chain-of-thought or hidden reasoning. Record the useful engineering rationale, assumptions, evidence, constraints and alternatives instead.

Keep the journal compact: when a newer entry supersedes an old status or priority, update/condense the old information so future chats do not waste context on obsolete plans.

## Cross-project rule

The owner wants this same journal discipline used for all of their software projects. When working in another repository, create or maintain an equivalent project journal and a repository-level handoff instruction there as well.
