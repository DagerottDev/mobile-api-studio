# Architecture

> **As-built status:** v0.5 domain features and the localhost migration are implemented. Owner-led device and migration validation remains open.

## 1. System overview

```text
┌──────────────────────────────────────────────────────────────┐
│ Browser at http://127.0.0.1:8180 (React, nine routes)        │
│ POST /api/invoke + process-lifetime header token             │
└──────────────────────────────┬───────────────────────────────┘
                               │
                  ┌────────────┴────────────┐
                  │ Axum + app-core crate   │
                  │ orchestration + state  │
                  └─┬─────┬─────┬─────┬────┘
                    │     │     │     │
        ┌───────────┘     │     │     └──────────────┐
        ▼                 ▼     ▼                    ▼
 Device adapters      Capture  Storage          Replay/Mocks
 simctl / ADB         engine   SQLite+bodies    Compare / AI
        │                 │
        │             mitmdump
        │                 │
        └──────────┬──────┘
                   ▼
        iOS Simulator / Android Emulator

Optional app-aware path:

iOS Swift SDK / Android Kotlin SDK
        -> local SDK ingestion
        -> persisted SDK events
        -> request correlation
        -> Traffic / Compare enrichment

Optional external AI path:

local deterministic evidence
        -> redaction + limits
        -> exact preview + fingerprint
        -> explicit user send
        -> provider adapter
        -> local AI result history
```

## 2. Architectural boundaries

### React UI

Owns rendering and transient interaction state.

It does not directly:

- execute ADB or `simctl`;
- spawn/stop mitmdump;
- write SQLite;
- manage certificate files;
- access Keychain secrets;
- call external AI providers directly.

### Localhost service and application core

`apps/local-server` serves the built UI, checks loopback Host and same-origin requests, and exposes an explicit command allowlist. `crates/app-core` owns state and all 71 task workflows, bridging the UI to focused Rust crates. Connection-changing operations are serialized across tabs:

- connect/disconnect;
- capture/session lifecycle;
- flow inspection/search;
- Replay;
- collections/environments;
- mocks/fixtures/breakpoints;
- SDK status/enrichment;
- comparison/diagnostics;
- AI preview/send/history;
- import/export and settings.

### Adapter/domain crates

External dependencies and deterministic logic are kept outside UI code so the implementation can evolve independently.

---

## 3. Rust crate responsibilities

### `core-model`

Shared capture/workspace domain types, normalization, application errors, and serialization contracts.

### `capture-core`

`CaptureEngine` interface and capture lifecycle/event contracts.

### `capture-mitm`

- launch/manage mitmdump;
- read normalized sidecar events;
- expose capture lifecycle through `CaptureEngine`;
- isolate mitmproxy-specific behavior from the rest of the app.

### `device-ios`

- detect `xcrun`/simctl availability;
- discover booted Simulators;
- install the development capture root CA;
- expose iOS capability/error information.

### `device-android`

- discover ADB emulators;
- read runtime metadata;
- read/apply/clear emulator proxy state;
- expose Android capability/error information.

### `storage`

- SQLite migrations;
- capture-session/flow/detail persistence;
- workspace sessions/search/collections/environments/preferences;
- content-addressed body-store integration.

### `workspace-core`

Reusable workspace logic such as interpolation, export/diagnostic data structures, and endpoint/workspace helpers.

### `secret-store`

OS credential-store abstraction. The current macOS implementation uses Keychain. Secret values are not persisted in normal SQLite fields.

### `replay`

Native async HTTP execution for editable Replay drafts. Replay does not depend on the browser/webview and therefore is not subject to browser CORS behavior.

### `mock-core`

Deterministic mock rule model and actions.

### `mock-storage`

Persistent mock rule storage in the application database.

### `mock-fixtures`

Reusable response-fixture persistence.

### `sdk-protocol`

Versioned platform-neutral handshake/context/log/network event model and the internal request-correlation header contract.

### `sdk-transport`

Local SDK ingestion transport. The desktop binds SDK telemetry to host loopback; iOS Simulator uses `127.0.0.1`, while Android Emulator reaches the host through `10.0.2.2`.

### `sdk-storage`

SDK client registry plus app/context/log/network-event persistence.

### `compare-core`

Deterministic session comparison:

- endpoint normalization/matching;
- repeated-call alignment;
- request/response diffs;
- JSON shape/type drift;
- timing/size deltas;
- missing/extra calls;
- duplicate/retry, slow-call, error-cluster, and waterfall diagnostics.

### `ai-core`

- provider-neutral AI interface;
- OpenAI Responses implementation;
- deterministic redaction;
- bounded context construction;
- context fingerprinting.

### `ai-storage`

Local AI result history keyed by target/task/provider/model/context fingerprint.

---

## 4. Capture data path

```text
mobile request
  -> runtime proxy route
  -> mitmproxy addon
  -> normalized sidecar event
  -> capture-mitm
  -> app-core ingestion
  -> SQLite metadata + body store
  -> Traffic queries
  -> React timeline / Inspector
```

Bodies are not repeatedly copied through every layer. Large content is stored by SHA-256 and fetched on demand.

## 5. Sidecar responsibilities

The mitmproxy addon currently handles more than capture framing because Phase 3 deliberately places live request/response mutation at the proxy seam.

Responsibilities include:

- normalized request/response capture events;
- mock-rule hot reload from a local rule document;
- mock response/status/header/body/latency/drop actions;
- request and response breakpoint pending/decision envelopes;
- correlation-ID extraction and stripping before upstream delivery;
- typed diagnostics when sidecar-side processing fails.

The desktop remains the source of truth for user-facing rules/state; the sidecar consumes published local state.

## 6. Connection and rollback model

Connection is treated as a transaction.

Conceptually:

```text
validate prerequisites
 -> start capture engine
 -> prepare CA
 -> configure selected runtime
 -> persist active capture session
 -> capture traffic
```

A rollback journal records device mutations that must survive an abnormal application exit.

Current platform behavior:

- **Android Emulator:** reads the prior global proxy, applies the Mobile API Studio proxy, and restores the previous value on disconnect/recovery.
- **iOS Simulator:** installs the local capture CA through `simctl`; proxy routing remains guided/manual rather than silently mutating broad macOS proxy configuration.

## 7. Storage architecture

SQLite stores structured metadata. Bodies are stored outside SQLite in a content-addressed tree such as:

```text
bodies/ab/cd/<sha256>.body
```

Major persisted domains include:

- sessions and flows;
- headers/details/body references;
- normalized endpoint index;
- saved collections/requests;
- environments and preferences;
- mock rules/fixtures;
- SDK clients/events;
- AI result history.

Secret environment variables and the AI API key use the OS credential store rather than normal database values.

## 8. Replay architecture

Replay is intentionally detached from mitmproxy.

```text
captured/saved request
 -> editable Replay draft
 -> environment interpolation
 -> protected internal-header removal
 -> native Rust HTTP execution
 -> replay response
 -> persisted flow with source = replay
```

This lets Replay work as an API-development surface even when no capture session is active.

## 9. Mock and breakpoint architecture

```text
Desktop rule/fixture state
 -> SQLite
 -> atomic mock-rules.json publish
 -> mitmproxy hot reload
 -> request/response mutation
 -> captured result tagged source = mock
```

Breakpoints use local file-backed envelopes under the isolated mitmproxy data directory:

```text
breakpoints/pending
breakpoints/decisions
```

A breakpoint has a bounded auto-continue timeout so unresolved UI state does not indefinitely deadlock emulator traffic.

## 10. App-aware SDK architecture

SDK enrichment is optional and does not replace proxy capture.

### Correlation

Instrumented requests receive an internal development request ID:

```text
X-Mobile-API-Studio-Request-Id
```

The SDK sends its network/context event locally. The proxy captures the request ID for local joining and removes the header before forwarding to the real backend.

The internal header is also omitted from normal cURL export, Replay, and AI context.

### SDK telemetry boundary

- iOS SDK telemetry uses an ephemeral URLSession configured not to use the app capture instrumentation/proxy path.
- Android SDK telemetry uses a direct local transport path rather than the intercepted OkHttp client.
- disabled SDK configuration leaves app requests unmodified.

## 11. Comparison architecture

Comparison builds a local `SessionSnapshot` for baseline and candidate sessions and sends those snapshots to `compare-core`.

Matching is deterministic:

```text
normalized endpoint
 + method
 + occurrence order within endpoint
```

The output is structured evidence, including presence, field differences, JSON shape drift, timing/size changes, SDK context differences, and per-session diagnostics.

No AI call is required to compute comparison results.

## 12. AI architecture

The AI path starts **after** local deterministic evidence exists.

```text
selected comparison/flow
 -> deterministic JSON evidence
 -> redact headers/query/body secret keys
 -> omit internal correlation metadata
 -> cap strings/body/context
 -> generate exact preview
 -> SHA-256 fingerprint
 -> explicit user send
 -> recompute + fingerprint check
 -> AIProvider
 -> local result history
```

The current provider is OpenAI through the Responses API with `store: false`.

Provider API keys are read from the OS secure store only when needed and are not copied into SQLite or workspace exports.

## 13. Frontend information architecture

Current top-level workspaces:

```text
Connect
Traffic
Replay
Mocks
SDK
Compare
AI
Workspace
Settings
```

The UI queries the same task-oriented command names through one typed HTTP adapter; it does not mirror the whole database into a single JavaScript state store. Each area has a stable local URL. The former Tauri frontend is retained temporarily for parity comparison and is not the release target.

## 14. Platform scope

### Implemented target

- macOS host with loopback-only browser UI;
- iOS Simulator;
- Android Emulator.

### Deferred

- physical mobile devices;
- Windows/Linux desktop builds;
- cloud/team synchronization;
- gRPC/HTTP3-specific tooling;
- public plugin marketplace.

## 15. Validation status

Architecture and product code through Phase 5 are implemented, but formal validation is intentionally outside the phase gates. See [FINAL_VALIDATION.md](FINAL_VALIDATION.md) for the owner-led validation stage.
