# Roadmap and Phase Gates

The roadmap is feature-gated, not date-gated. Formal testing/validation is intentionally deferred until after implementation Phases 0–5, per `AGENTS.md`; the checkboxes below track implementation status only.

## Phase 0 — Foundation

**Goal:** prove the internal pipeline.

### Work

- [x] Tauri 2 + React + TypeScript shell
- [x] Cargo/pnpm workspace
- [x] core model crate
- [x] SQLite migrations
- [x] body storage abstraction
- [x] local fixture/fake capture foundations
- [x] fake capture event generator
- [ ] CI — deferred by project rule

### Implementation status

Implemented and merged to `main`. Formal verification is deferred.

---

## Phase 1 — v0.1 Capture / Inspect / Replay

### Device

- [x] iOS Simulator discovery
- [x] Android Emulator discovery
- [x] platform capability probe
- [x] connection coordinator + rollback

### Capture

- [x] CaptureEngine trait
- [x] mitmdump adapter
- [x] CA lifecycle
- [x] iOS connection strategy
- [x] Android connection strategy
- [x] connection diagnostics

### UI

- [x] traffic timeline
- [x] request/response inspector
- [x] body rendering foundations
- [x] host/method/status filters

### Productivity

- [x] safe cURL export
- [x] replay draft
- [x] replay execution
- [x] replay result stored in session

### Implementation status

Implemented and merged to `main`. Formal verification is deferred.

---

## Phase 2 — v0.2 Daily Debugger

- [x] persistent named sessions
- [x] advanced filters/search
- [x] endpoint normalization
- [x] saved request collections
- [x] environment variables
- [x] OS-secure secret variables
- [x] export/import
- [x] Connection Doctor
- [x] managed/custom capture sidecar path
- [x] better setup onboarding

### Implementation status

Implemented and merged to `main`. Formal verification is deferred.

---

## Phase 3 — v0.3 Mocking

- [x] mock rule engine
- [x] create mock from captured flow
- [x] status override
- [x] response body override
- [x] latency
- [x] timeout/drop
- [x] response mutation
- [x] request/response breakpoint
- [x] fixtures
- [x] “disable all mocks” safety action

### Implementation status

Implemented and merged to `main` in PR #11. Formal verification is deferred.

---

## Phase 4 — v0.4 App-Aware SDK

### iOS

- [x] Swift Package
- [x] URLSession integration
- [x] manual/custom client instrumentation API
- [x] context/log events
- [x] source metadata helpers
- [x] debug-first/no-op release behavior
- [x] sample app

### Android

- [x] Kotlin core
- [x] OkHttp interceptor
- [x] manual/custom client instrumentation API
- [x] context/log events
- [x] source metadata helpers
- [x] no-op release strategy
- [x] sample app

### Desktop

- [x] versioned local SDK transport
- [x] app/device handshake and client registry
- [x] SDK event persistence
- [x] proxy↔SDK correlation
- [x] local-only correlation-header stripping
- [x] session app attribution
- [x] SDK health/connection diagnostics
- [x] source/screen/feature/log metadata UI
- [x] app-context flow search

### Implementation status

Implemented and merged to `main` in PR #13. Formal verification is deferred.

---

## Phase 5 — v0.5 Compare + AI

### Diff

- [x] session pairing
- [x] endpoint matching
- [x] repeated-call alignment
- [x] request method/query/header/body diff
- [x] response status/header/body diff
- [x] timing and size diff
- [x] missing/extra call detection
- [x] JSON shape/type drift
- [x] SDK app/screen/feature/source context comparison

### Deterministic diagnostics

- [x] duplicate/retry detection
- [x] slowest requests
- [x] waterfall overlap/sequential groups
- [x] error clustering
- [x] evidence-count comparison summary

### Desktop

- [x] Compare workspace route
- [x] baseline/candidate selectors
- [x] endpoint and occurrence drill-down
- [x] request/response/timing diff inspector
- [x] missing/extra/drift indicators
- [x] deterministic diagnostics panel
- [x] SDK context difference display

### AI

- [x] provider-neutral interface
- [x] OpenAI Responses API provider
- [x] BYOK OS-secure storage
- [x] provider/model/settings UI
- [x] deterministic redaction pipeline
- [x] exact external-context preview + SHA-256 fingerprint gate
- [x] session-diff explanation
- [x] selected-flow diagnosis
- [x] local AI result history with provider/model/context fingerprint
- [x] optional AI path that never blocks deterministic comparison

### Privacy

- [x] sensitive header redaction
- [x] internal correlation-header omission
- [x] configurable JSON/query secret-key redaction
- [x] body/string/context limits
- [x] explicit user send action after preview
- [x] OpenAI `store: false`

### Implementation status

Implementation complete on `phase-5/compare-ai`; integration to `main` remains. Formal verification is deferred.

### Exit outcome

A developer can explain a meaningful iOS/Android mismatch using deterministic diffing, with AI as an optional enhancement.

---

# Deferred backlog

Intentionally not scheduled before v0.5:

- physical iOS/Android devices
- Windows/Linux desktop builds
- team/cloud synchronization
- gRPC inspector
- HTTP/3-specific tooling
- OpenAPI generation/import depth
- test generation
- CI/headless mode
- plugin marketplace
- production APM integration
- traffic sharing service
