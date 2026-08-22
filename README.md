# Mobile API Studio

A local-first desktop debugger for inspecting, replaying, mocking, correlating, and comparing API traffic from iOS Simulators and Android Emulators.

> **Status:** implementation Phases 0–5 are complete and merged to `main`. The repository remains private and formal testing/CI/final validation are intentionally deferred to the repository owner.

## What is implemented

Mobile API Studio now covers the full planned v0.5 workflow:

1. Discover booted iOS Simulators and Android Emulators.
2. Connect a runtime to a local capture session.
3. Capture and inspect HTTP(S) traffic in a searchable timeline.
4. Persist sessions, request/response details, and content-addressed bodies locally.
5. Copy a secret-redacted cURL command or turn a captured request into an editable Replay draft.
6. Save reusable requests into collections and resolve local environments, including Keychain-backed secrets.
7. Mock responses, inject latency/errors, reuse fixtures, and pause request/response breakpoints.
8. Add optional iOS/Android SDK context such as app, screen, feature, source location, and logs.
9. Compare two sessions deterministically for missing/extra calls, request/response differences, JSON shape drift, retries, errors, and timing regressions.
10. Optionally ask an AI provider to explain selected, redacted comparison/flow evidence after an explicit context preview.

The product is intentionally not a generic Postman replacement. Its differentiator is **mobile runtime awareness**: device discovery, capture orchestration, app-aware context, failure simulation, and iOS↔Android comparison.

## Current stack

- **Desktop shell:** Tauri 2
- **UI:** React + TypeScript + Vite
- **Core/runtime:** Rust + Tokio
- **Metadata:** SQLite
- **Body storage:** content-addressed SHA-256 file store
- **Capture:** mitmproxy/mitmdump sidecar behind the Rust `CaptureEngine` boundary
- **iOS runtime integration:** `xcrun simctl`
- **Android runtime integration:** ADB
- **Replay:** native Rust HTTP client
- **iOS app-aware SDK:** Swift Package
- **Android app-aware SDK:** Kotlin library + OkHttp interceptor
- **AI:** provider-neutral Rust interface with OpenAI Responses API implementation
- **Secrets:** macOS Keychain through the secure-store abstraction

## Implementation phases

| Phase | Version | Status | Outcome |
|---|---|---|---|
| 0 | Foundation | ✅ Merged | Tauri/Rust/React foundation, domain models, SQLite/body storage, capture abstractions |
| 1 | v0.1 | ✅ Merged | Simulator/emulator discovery, capture, inspect, safe cURL, Replay |
| 2 | v0.2 | ✅ Merged | Sessions, search, collections, environments, import/export, Connection Doctor |
| 3 | v0.3 | ✅ Merged | Mock rules, fixtures, latency/errors, request/response breakpoints |
| 4 | v0.4 | ✅ Merged | Swift/Kotlin SDKs, app context, logs, proxy↔SDK correlation |
| 5 | v0.5 | ✅ Merged | Session comparison, deterministic diagnostics, optional redacted AI debugging |

Formal validation is a separate owner-led stage and has not been performed as part of these implementation phases.

See [docs/ROADMAP.md](docs/ROADMAP.md) for the phase record and [docs/FINAL_VALIDATION.md](docs/FINAL_VALIDATION.md) for the deferred validation checklist.

## As-built repository layout

```text
mobile-api-studio/
├── apps/
│   └── desktop/
│       ├── src/                  # React workspaces and UI
│       └── src-tauri/            # Tauri commands/orchestration
├── crates/
│   ├── core-model/               # shared capture/workspace models
│   ├── capture-core/             # CaptureEngine interface
│   ├── capture-mitm/             # mitmdump process/event bridge
│   ├── device-ios/               # simctl integration
│   ├── device-android/           # ADB/emulator integration
│   ├── storage/                  # SQLite + body store
│   ├── replay/                   # native request replay
│   ├── workspace-core/           # interpolation/diagnostic helpers
│   ├── secret-store/             # OS credential-store abstraction
│   ├── mock-core/                # deterministic mock rules
│   ├── mock-storage/             # mock persistence
│   ├── mock-fixtures/            # reusable mock fixtures
│   ├── sdk-protocol/             # versioned app-aware SDK events
│   ├── sdk-storage/              # SDK event/client persistence
│   ├── sdk-transport/            # local SDK ingestion server
│   ├── compare-core/             # deterministic session comparison
│   ├── ai-core/                  # redaction + provider abstraction
│   └── ai-storage/               # local AI result history
├── sidecars/
│   └── mitm-addon/               # mitmproxy capture/mock/breakpoint bridge
├── sdks/
│   ├── ios/                      # Swift Package
│   └── android/                  # Kotlin/OkHttp library + sample
├── samples/
│   └── ios-sdk-demo/             # iOS SDK sample integration
└── docs/
```

## Core principles

- **Local-first:** capture sessions and debugging data stay on the developer machine by default.
- **Safe by default:** secret headers, internal correlation metadata, and configured secret keys are redacted from exports and AI context.
- **Explicit AI boundary:** AI is optional; the user previews the sanitized context before an external request is allowed.
- **No pinning bypass feature:** pinned clients require an app-owned debug configuration or the optional SDK path.
- **Reversible mutations:** device/proxy changes are journaled and rolled back where the selected strategy changes them.
- **Adapter boundaries:** capture, device, replay, mocks, SDK transport, comparison, secure storage, and AI providers remain replaceable components.
- **Deterministic before AI:** comparisons and diagnostics are computed locally first; AI explains evidence rather than becoming the source of truth.

## Documentation

- [Implementation record](docs/IMPLEMENTATION_PLAN.md)
- [Architecture](docs/ARCHITECTURE.md)
- [Roadmap and phase status](docs/ROADMAP.md)
- [Final owner-led validation](docs/FINAL_VALIDATION.md)
- [Security and privacy](docs/SECURITY_AND_PRIVACY.md)
- [SDK integration](docs/SDK_INTEGRATION.md)
- [AI privacy and providers](docs/AI_PRIVACY_AND_PROVIDERS.md)
- [Issue backlog / implementation history](docs/ISSUE_BACKLOG.md)
- [Research notes](docs/RESEARCH_NOTES.md)
- [ADRs](docs/adr/)

## Current next step

The planned product implementation is complete through v0.5. The next project stage is **independent final validation by the repository owner**: build/run the desktop app, exercise supported iOS/Android workflows, record defects, and fix only issues discovered during that validation cycle.

No automated test or CI program is being introduced automatically as part of this documentation refresh.

## License

No public license has been selected yet. The repository remains private until that decision and the release/security review are complete.
