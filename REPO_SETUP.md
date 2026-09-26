# Repository Setup and Current State

The GitHub repository already exists and the planned implementation through v0.5 is merged to `main`.

## Repository

- Name: `mobile-api-studio`
- Visibility: public.
- Default branch: `main`
- Description: `Local-first API debugger for iOS Simulators and Android Emulators: capture, replay, mock, compare.`
- License: Apache-2.0

## Current state

- Phases 0–5: implemented and merged.
- Current implementation milestone: localhost source-build migration on top of v0.5.
- Focused local API tests exist; device validation and release approval remain with the repository owner. No CI gate has been added.
- Public source preparation: README, contribution guide, funding links, and Apache-2.0 licensing are included. macOS binary distribution remains a separate signed/notarized release decision.

## New-machine development setup

The current source build uses a loopback-only Axum service with a React browser UI. A development machine should have the platform tooling needed for the workflows it intends to exercise:

```text
Rust toolchain compatible with workspace rust-version
Node.js + pnpm
Xcode + Command Line Tools for iOS Simulator workflows
Android SDK Platform Tools / ADB for Android Emulator workflows
mitmproxy/mitmdump or the configured capture executable
```

Run `pnpm install --frozen-lockfile` and `./scripts/run-local.sh` from the repository root. The service opens `http://127.0.0.1:8180`; it can store a custom mitmdump executable path in Settings. Close the historical Tauri app before using the shared data directory.

## Runtime-specific notes

### iOS Simulator

- `xcrun simctl` is used for discovery and development CA installation.
- Simulator proxy routing is guided/manual rather than silently changing broad macOS proxy settings.
- HTTPS interception may require explicitly enabling full trust for the development CA.

### Android Emulator

- ADB is used for discovery and emulator proxy configuration.
- The emulator reaches the development host through the standard emulator host alias (`10.0.2.2`) where applicable.
- Debug app network-security configuration may need to trust user-added CAs.

## Secrets

- secret environment variables use the OS secure-store abstraction;
- the optional OpenAI/BYOK API key also uses secure storage;
- normal workspace exports do not contain those secret values.

## Next project stage

The next stage is the repository owner's independent localhost and device validation cycle described in [docs/FINAL_VALIDATION.md](docs/FINAL_VALIDATION.md).
