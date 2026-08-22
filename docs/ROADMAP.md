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

Implementation complete on `phase-4/app-aware-sdk`; integration to `main` remains. Formal verification is deferred.

---

## Phase 5 — v0.5 Compare + AI

### Diff

- [ ] session pairing
- [ ] endpoint matching
- [ ] request diff
- [ ] response diff
- [ ] timing diff
- [ ] missing/extra call detection
- [ ] JSON shape drift

### Deterministic diagnostics

- [ ] duplicate/retry detection
- [ ] slowest requests
- [ ] waterfall overlap/sequential groups
- [ ] error clustering

### AI

- [ ] provider interface
- [ ] BYOK secure storage
- [ ] redaction pipeline
- [ ] context preview
- [ ] session-diff explanation
- [ ] selected-flow diagnosis

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
