# Mobile API Studio

A local-first desktop debugger for inspecting, replaying, mocking, and comparing API traffic from iOS Simulators and Android Emulators.

> Status: planning / pre-alpha. The repository is intentionally private until the first usable capture workflow is stable.

## Product vision

Mobile API Studio combines the best parts of a mobile network proxy, an API client, and app-aware debugging into one workflow:

1. Discover a booted iOS Simulator or Android Emulator.
2. Connect it to a capture session.
3. Inspect HTTP(S) traffic in a searchable timeline.
4. Promote a captured request into an editable request and replay it.
5. Mock or alter responses without waiting for backend changes.
6. Compare iOS and Android behavior for the same endpoint or user flow.
7. Optionally add a lightweight app SDK for code-level context and pinned-network stacks.

The long-term goal is not to become another generic Postman clone. The differentiator is **mobile runtime awareness**: device discovery, one-click connection, capture sessions, app correlation, mobile-specific failure simulation, and cross-platform comparison.

## Primary stack

- **Desktop shell:** Tauri 2
- **UI:** React + TypeScript + Vite
- **Core:** Rust + Tokio
- **Metadata store:** SQLite
- **Capture adapter (initial):** mitmproxy/mitmdump sidecar behind a Rust `CaptureEngine` interface
- **iOS integration:** `xcrun simctl`
- **Android integration:** ADB + emulator-aware connection strategies
- **Optional SDKs later:** Swift Package + Kotlin/Android library

## Development phases

| Phase | Version | Outcome |
|---|---|---|
| 0 | Foundation | Repo, architecture, shell, CI, models, local test server |
| 1 | v0.1 | Detect simulator/emulator, capture HTTP(S), inspect, persist, copy cURL, replay |
| 2 | v0.2 | Search, filters, sessions, collections, environments, connection diagnostics |
| 3 | v0.3 | Mock responses, latency/errors, breakpoints, fixture-based testing |
| 4 | v0.4 | iOS + Android SDKs, app/screen/source correlation, richer traces |
| 5 | v0.5 | iOS↔Android comparison, schema drift detection, opt-in AI debugging |

See [docs/IMPLEMENTATION_PLAN.md](docs/IMPLEMENTATION_PLAN.md) for the detailed plan and [docs/ROADMAP.md](docs/ROADMAP.md) for phase gates.

## Core principles

- **Local-first:** captured traffic stays on the developer's machine by default.
- **Safe by default:** secrets are redacted in UI exports and AI payloads unless explicitly revealed.
- **No pinning bypass feature:** the product does not silently defeat certificate pinning. For apps you control, use the optional SDK or debug network configuration.
- **Reversible device changes:** proxy/certificate changes must be tracked and restored where technically possible.
- **Adapter-based architecture:** proxy, device, persistence, replay, mock, and AI capabilities are replaceable components.
- **Useful before clever:** ship capture/replay before AI or advanced protocol support.

## Planned repository layout

```text
mobile-api-studio/
├── apps/
│   └── desktop/
│       ├── src/                  # React UI
│       └── src-tauri/            # Tauri app entrypoint
├── crates/
│   ├── core-model/               # shared domain models
│   ├── capture-core/             # CaptureEngine interfaces
│   ├── capture-mitm/             # mitmdump adapter/process bridge
│   ├── device-ios/               # simctl integration
│   ├── device-android/           # adb/emulator integration
│   ├── storage/                  # SQLite + body store
│   ├── replay/                   # request replay engine
│   ├── mock-engine/              # v0.3
│   └── diff-engine/              # v0.5
├── sidecars/
│   └── mitm-addon/               # flow->JSON event bridge
├── sdk/
│   ├── ios/                      # v0.4 Swift Package
│   └── android/                  # v0.4 Kotlin library
├── fixtures/
│   └── test-server/              # deterministic local APIs
├── docs/
└── .github/
```

## First milestone

The first meaningful demo is deliberately small:

> Launch Mobile API Studio → select a booted simulator/emulator → connect → open a sample app → see requests arrive → select one request → view headers/body/timings → copy a redacted cURL → edit and replay the request.

No mocking, AI, GraphQL tooling, team sync, or production-device support is required for v0.1.

## Documentation

- [Detailed implementation plan](docs/IMPLEMENTATION_PLAN.md)
- [Architecture](docs/ARCHITECTURE.md)
- [Roadmap and phase gates](docs/ROADMAP.md)
- [Security and privacy](docs/SECURITY_AND_PRIVACY.md)
- [Issue backlog](docs/ISSUE_BACKLOG.md)
- [Research notes](docs/RESEARCH_NOTES.md)
- [ADRs](docs/adr/)

## License

No public license has been selected yet. Keep the repository private until that decision is made.
