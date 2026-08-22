# Implementation Backlog Record

This file began as the 71-item seed backlog used to structure Phases 0–5. Those implementation phases are now complete and merged to `main`.

It is retained as a compact implementation-history map rather than a list of work still waiting to be started.

## Phase 0 — Foundation — implemented

Original scope covered:

- Tauri 2 + React desktop scaffold;
- Cargo/pnpm workspace;
- shared domain models;
- SQLite persistence;
- content-addressed body storage;
- fake/local capture event path;
- capture abstraction.

The original CI/testing items were deferred by repository-owner policy.

## Phase 1 — v0.1 Capture / Inspect / Replay — implemented

Original issue range: 9–28.

Implemented scope includes:

- `CaptureEngine` and mitmdump bridge/process lifecycle;
- iOS Simulator and Android Emulator discovery;
- CA/proxy handling and rollback journal;
- typed connection diagnostics;
- live Traffic and Inspector;
- default secret redaction and safe cURL;
- Replay draft/editor/native execution;
- persisted replay results.

Performance/E2E test issues from the seed backlog were deferred to final owner-led validation rather than being implemented as automated test infrastructure.

## Phase 2 — v0.2 Daily Debugger — implemented

Original issue range: 29–38.

Implemented scope includes:

- named sessions;
- cross-session search/filtering;
- endpoint normalization;
- saved request collections;
- environment interpolation;
- OS-secure secret variables;
- workspace import/export;
- Connection Doctor/onboarding;
- configurable/managed capture-sidecar executable path.

## Phase 3 — v0.3 Mocking — implemented

Original issue range: 39–48.

Implemented scope includes:

- persistent mock rules;
- mock creation from captured traffic;
- static/mutated responses;
- latency/status/header/body/JSON overrides;
- timeout/drop simulation;
- reusable fixtures;
- request/response breakpoints;
- global “Disable all mocks” action.

## Phase 4 — v0.4 App-Aware SDK — implemented

Original issue range: 49–59.

Implemented scope includes:

- versioned local SDK protocol;
- Swift Package and URLSession/manual instrumentation;
- Kotlin SDK and OkHttp/manual instrumentation;
- sample iOS/Android integrations;
- debug/pass-through enable behavior;
- local SDK ingestion and client registry;
- proxy↔SDK correlation;
- app/screen/feature/source/log enrichment.

## Phase 5 — v0.5 Compare + AI — implemented

Original issue range: 60–71.

Implemented scope includes:

- session pairing;
- deterministic endpoint/call alignment;
- request/response/timing diffs;
- JSON shape/type drift;
- missing/extra request detection;
- duplicate/retry, slow-call, error-cluster, and waterfall diagnostics;
- provider-neutral AI interface;
- redaction/context preview and fingerprint gate;
- optional OpenAI Responses provider;
- local AI result history.

## Canonical phase trackers / integration PRs

The implementation was ultimately tracked by canonical phase issues/PRs rather than by creating all 71 seed issues individually.

Notable integration records include:

- Phase 3: PR #11
- Phase 4: issue #12 / PR #13
- Phase 5: issue #14 / PR #15

The roadmap is the canonical high-level implementation status: [ROADMAP.md](ROADMAP.md).

## Current backlog category: final validation defects

No implementation phase is currently open. During the owner-led validation stage, new issues should represent **observed defects or concrete release blockers**, for example:

```text
Validation: Android proxy is not restored after failed connection
Validation: iOS CA status guidance is incorrect on runtime X
Validation: Replay corrupts binary request body
Validation: mock breakpoint does not auto-continue
Validation: SDK context fails to correlate with a captured flow
Validation: comparison aligns repeated calls incorrectly
Validation: AI preview leaks configured secret key
```

Each issue should contain the runtime/workflow used to reproduce the problem and the expected vs actual behavior.

## Deferred post-v0.5 product backlog

These remain intentionally outside the completed v0.5 implementation scope:

- physical iOS devices;
- physical Android devices;
- Windows/Linux desktop builds;
- team/cloud synchronization;
- gRPC inspector;
- HTTP/3/QUIC-specific tooling;
- deeper OpenAPI workflows;
- public plugin marketplace;
- production APM integration;
- hosted traffic sharing;
- public release/update infrastructure.

Automated testing/CI is also not automatically scheduled by this backlog; the repository owner controls the final validation/testing strategy.
