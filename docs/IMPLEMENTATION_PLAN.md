# Mobile API Studio — Implementation Record

> **Status:** planned implementation Phases 0–5 are complete and merged to `main`. This document now records the as-built product scope rather than acting as a future implementation checklist. Formal testing, CI, benchmarks, and final validation remain deferred to the repository owner.

The original pre-build implementation plan remains available in Git history if historical sequencing detail is needed.

## 1. Product outcome

Mobile API Studio is a local-first desktop debugging environment for mobile API traffic. It combines runtime discovery, capture, inspection, replay, saved requests/environments, mocking, app-aware SDK context, cross-session comparison, and optional AI explanations.

The product is intentionally optimized for the workflow:

```text
mobile runtime
  -> capture
  -> inspect
  -> replay/mock
  -> correlate with app context
  -> compare sessions
  -> optionally explain redacted evidence with AI
```

It is not intended to be a general-purpose packet analyzer, cloud API collaboration platform, production APM service, or certificate-pinning bypass tool.

## 2. Architecture decisions that shipped

### Desktop and core

- Tauri 2 desktop shell.
- React + TypeScript + Vite UI.
- Rust/Tokio orchestration and domain logic.
- SQLite metadata persistence.
- Content-addressed SHA-256 body storage.

### Capture

- Stable Rust `CaptureEngine` abstraction.
- `mitmdump`/mitmproxy sidecar as the first concrete capture engine.
- Versioned normalized sidecar events.
- Reversible connection/rollback handling.
- iOS Simulator discovery and CA installation through `simctl`.
- Android Emulator discovery and proxy handling through ADB.

### Security boundaries

- Secret-aware headers and exports.
- OS-secure environment/API-key storage.
- No automatic certificate-pinning bypass.
- Local-only SDK telemetry transport.
- Internal request-correlation headers removed before real backend delivery and omitted from replay/cURL/AI context.

### Extensibility

The main internal seams are:

```text
CaptureEngine
DeviceProvider
Replay engine
Mock rule engine
SDK protocol/transport
Compare engine
SecretStore
AIProvider
```

These remain internal extension points; a public plugin marketplace/API is still deferred.

---

# 3. Phase record

## Phase 0 — Foundation — complete

Delivered:

- Tauri/React desktop shell.
- Cargo + pnpm monorepo structure.
- shared Rust models and typed application errors.
- SQLite database foundation.
- content-addressed body store.
- fake/local capture event path for UI/storage integration.
- `CaptureEngine` boundary.

Testing/CI work originally mentioned in early planning was explicitly deferred by project rule.

## Phase 1 — v0.1 Capture / Inspect / Replay — complete

Delivered:

- booted iOS Simulator discovery with `simctl`.
- Android Emulator discovery with ADB.
- mitmdump process lifecycle and normalized capture bridge.
- CA/proxy connection orchestration and rollback journal.
- Traffic timeline and request/response inspector.
- persisted flow details and body references.
- safe secret-redacted cURL export.
- native Rust request Replay with editable drafts and persisted replay results.
- typed connection diagnostics.

## Phase 2 — v0.2 Daily Debugger — complete

Delivered:

- named historical sessions and session metadata.
- cross-session traffic search and filters.
- normalized endpoint indexing.
- request collections and saved requests.
- environments with `{{variable}}` interpolation.
- Keychain-backed secret variables.
- versioned workspace import/export with secrets excluded.
- Connection Doctor/onboarding.
- configurable/managed mitmdump executable path.

## Phase 3 — v0.3 Mocking — complete

Delivered:

- persistent ordered mock rules.
- quick mock creation from captured traffic.
- status, body, header, JSON mutation, latency, timeout, and drop actions.
- reusable response fixtures.
- request/response breakpoints with file-backed pause/decision envelopes.
- bounded breakpoint timeout behavior.
- mocked traffic classified as `mock`.
- global “Disable all mocks” safety action.

## Phase 4 — v0.4 App-Aware SDK — complete

Delivered:

### iOS

- Swift Package.
- explicit/manual request instrumentation.
- opt-in URLSession/`URLProtocol` instrumentation.
- screen/feature/context/log events.
- source file/function/line metadata helpers.
- disabled/no-op behavior unless explicitly enabled.
- sample iOS app.

### Android

- Kotlin SDK.
- OkHttp interceptor.
- manual/custom-client instrumentation.
- context/log/source metadata.
- disabled pass-through behavior.
- sample Android app.

### Desktop

- versioned SDK event protocol.
- local ingestion service on port `8182`.
- SDK client registry and event persistence.
- proxy↔SDK request correlation.
- app/session attribution.
- SDK health/status workspace.
- SDK-aware Traffic search and Inspector enrichment.

## Phase 5 — v0.5 Compare + AI — complete

Delivered:

### Deterministic comparison

- baseline/candidate session pairing.
- normalized endpoint matching.
- repeated-call alignment by deterministic occurrence order.
- method/query/header/request-body diff.
- response status/header/body diff.
- timing and response-size deltas.
- missing/extra call detection.
- JSON shape/type drift detection.
- SDK app/screen/feature/source comparison.

### Diagnostics

- duplicate/retry candidates.
- slowest-request ranking.
- error clusters.
- sequential vs overlapping waterfall evidence.
- high-level evidence counts.

### Optional AI

- provider-neutral `AIProvider` abstraction.
- OpenAI Responses API implementation.
- BYOK API key in OS credential storage.
- provider/model/redaction settings UI.
- sanitized context preview before sending.
- SHA-256 fingerprint gate ensuring sent context matches previewed context.
- session-diff explanation.
- selected-flow diagnosis.
- local AI result history with provider/model/context fingerprint.
- OpenAI requests use `store: false`.

AI remains optional and never blocks local deterministic comparison.

---

# 4. As-built data model

Core data is split across focused stores/models rather than a single monolith.

Primary concepts:

```text
CaptureSession
FlowSummary
FlowDetail
RequestDetail
ResponseDetail
Timing
HeaderValue
BodyRef
NormalizedEndpoint
SavedCollection
SavedRequest
Environment
EnvironmentVariable
MockRule
MockFixture
SdkEnvelope
SdkClientRecord
SessionComparison
AiResultRecord
```

Bodies are addressed by SHA-256 and loaded on demand instead of repeatedly copied through UI events.

---

# 5. As-built desktop workspaces

The desktop application currently exposes these product surfaces:

```text
Connect     runtime discovery and connection lifecycle
Traffic     search, timeline, Inspector, cURL, saved-request/mock/fixture actions
Replay      captured/saved request editing and native resend
Mocks       rule editor, fixtures, and live breakpoints
SDK         connected SDK clients, app events, health, attribution
Compare     baseline/candidate deterministic comparison and diagnostics
AI          preview-gated optional external explanations and local history
Workspace   sessions, collections, environments
Settings    Connection Doctor, import/export, sidecar and AI configuration
```

---

# 6. Performance and storage design

Engineering targets remain:

- keep large bodies out of high-frequency UI event payloads;
- lazy-load request/response bodies;
- virtualize or bound large traffic collections in the UI;
- keep metadata queryable through SQLite indexes;
- retain body truncation/encoding metadata;
- avoid unbounded SDK, breakpoint, mock, or AI context payloads.

The original 10k-flow goal should be evaluated during final owner-led validation rather than treated as already verified.

---

# 7. Security and privacy implementation

Implemented boundaries include:

- secret-redacted cURL by default;
- secret-aware workspace exports;
- Keychain-backed secret environment variables;
- Keychain-backed AI API key;
- local SDK transport;
- correlation-header stripping before upstream delivery;
- configurable AI secret-key redaction for JSON/query data;
- context/body/string limits before AI requests;
- explicit AI preview and user send action;
- no certificate-pinning bypass capability.

See [SECURITY_AND_PRIVACY.md](SECURITY_AND_PRIVACY.md) and [AI_PRIVACY_AND_PROVIDERS.md](AI_PRIVACY_AND_PROVIDERS.md).

---

# 8. Deferred scope

Still intentionally outside the completed v0.5 implementation scope:

- physical iOS/Android device workflows;
- Windows/Linux desktop packaging;
- team/cloud synchronization;
- gRPC tooling;
- HTTP/3-specific tooling;
- deep OpenAPI generation/import;
- plugin marketplace;
- production APM integration;
- hosted traffic sharing;
- public release/update infrastructure.

---

# 9. Final validation stage

Implementation is complete, but it is **not claimed as fully validated**.

Per repository working rules:

- automated tests were not written during Phases 0–5;
- CI/test workflows were not used as phase gates;
- final validation is performed independently by the repository owner after implementation.

The next stage is documented in [FINAL_VALIDATION.md](FINAL_VALIDATION.md). Defects discovered there should be tracked as validation/fix issues rather than reopening the original phase implementation plan.

---

# 10. Release readiness after validation

Before considering a public/open-source release, separately decide and review:

- license;
- third-party/bundled sidecar licensing;
- security reporting process (`SECURITY.md`);
- CA/key threat model and uninstall behavior;
- packaging/notarization/update strategy;
- supported macOS/Xcode/Android runtime matrix;
- privacy wording for optional AI providers;
- final validation results and known limitations.
