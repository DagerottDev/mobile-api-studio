# Roadmap and Phase Gates

The roadmap is feature-gated, not date-gated. A phase is complete only when its exit criteria are demonstrated on fixture apps.

## Phase 0 — Foundation

**Goal:** prove the internal pipeline.

### Work

- [ ] Tauri 2 + React + TypeScript shell
- [ ] Cargo/pnpm workspace
- [ ] core model crate
- [ ] SQLite migrations
- [ ] body storage abstraction
- [ ] local fixture API server
- [ ] fake capture event generator
- [ ] CI

### Exit gate

Fake flow appears live in UI, persists, and reopens from SQLite.

---

## Phase 1 — v0.1 Capture / Inspect / Replay

### Device

- [ ] iOS Simulator discovery
- [ ] Android Emulator discovery
- [ ] platform capability probe
- [ ] connection coordinator + rollback

### Capture

- [ ] CaptureEngine trait
- [ ] mitmdump adapter
- [ ] CA lifecycle
- [ ] iOS connection strategy
- [ ] Android connection strategy
- [ ] connection verification

### UI

- [ ] live virtualized timeline
- [ ] request/response inspector
- [ ] JSON/text/image body rendering
- [ ] basic host/method/status filters

### Productivity

- [ ] safe cURL export
- [ ] replay draft
- [ ] replay execution
- [ ] replay result stored in session

### Exit gate

Real HTTPS traffic from supported iOS Simulator and Android Emulator fixture apps can be captured and replayed reliably.

---

## Phase 2 — v0.2 Daily Debugger

- [ ] persistent named sessions
- [ ] advanced filters/search
- [ ] endpoint normalization
- [ ] saved request collections
- [ ] environment variables
- [ ] OS-secure secret variables
- [ ] export/import
- [ ] Connection Doctor
- [ ] managed/bundled capture sidecar
- [ ] better setup onboarding

### Exit gate

A developer can use the tool repeatedly without redoing manual setup and can find/reuse previous traffic quickly.

---

## Phase 3 — v0.3 Mocking

- [ ] mock rule engine
- [ ] create mock from captured flow
- [ ] status override
- [ ] response body override
- [ ] latency
- [ ] timeout/drop
- [ ] response mutation
- [ ] request/response breakpoint
- [ ] fixtures
- [ ] “disable all mocks” safety action

### Exit gate

The fixture mobile apps can be tested against common failure states without backend changes.

---

## Phase 4 — v0.4 App-Aware SDK

### iOS

- [ ] Swift Package
- [ ] URLSession integration
- [ ] manual/custom client instrumentation API
- [ ] context/log events
- [ ] sample app

### Android

- [ ] Kotlin core
- [ ] OkHttp interceptor
- [ ] context/log events
- [ ] no-op release strategy
- [ ] sample app

### Desktop

- [ ] local SDK transport
- [ ] app/device handshake
- [ ] proxy↔SDK correlation
- [ ] source/screen/feature metadata UI

### Exit gate

The same request can show both network data and developer-supplied app context.

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

### Exit gate

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
