# Roadmap checkpoint — 2026-10-02

The complete eight-milestone roadmap remains authorized. Separate milestone worktrees and stacked draft PRs preserve the original macOS release checkout and its validation data.

Milestones 1–5 have reviewable source changes (draft PRs #19–#23). Milestone 6 scripting and local automation now passes 19 app-core checks, worker limits, four private-socket checks, CLI/MCP self-check and real lifecycle, frontend production build, real HTTP/WebSocket hooks including failure/drop/count limits, and the existing network-condition live regression. See SCRIPTING_AUTOMATION.md. Milestone 5's 18 core checks, replay redirect checks, frontend and workspace compilation remain recorded in COMPOSE_INTERCHANGE.md. Native Charles/Proxyman imports remain conditional on documented versions and representative fixtures; unsupported formats recommend HAR.

Milestone 7 sharing is being integrated separately and milestone 8 platform support is staged. No external deployment or real traffic upload has occurred. Sharing uses self-hosted local verification until a hosting target is provided. No host-wide proxy settings were changed.

Acceptance gaps are explicit: no connected physical device or booted simulator was discovered; browser interaction is blocked by tool security policy; Windows/Linux runtime, credential-store, packaging and full recovery matrices remain unverified. Source implementation does not close those release gates.

Use only latest verified Luna/Sol subagents, bounded validated Jev decisions and relevant deterministic checks. Check native usage regularly and preserve a checkpoint at the applicable allowance limit.
