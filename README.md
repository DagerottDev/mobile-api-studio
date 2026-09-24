# Mobile API Studio

**A local-first API debugger for iOS Simulators and Android Emulators.** Capture a request, inspect what happened, replay it, mock its response, and compare two sessions from one macOS desktop app.

[Build from source](#build-from-source) · [How capture works](#how-capture-works) · [Contribute](CONTRIBUTING.md) · [Support the project](#support-the-project)

> **Release status:** The planned v0.5 feature set is implemented. An Apple Silicon macOS app has been built and launched, but device workflows and the [final validation checklist](docs/FINAL_VALIDATION.md) are still in progress. There is no notarized public download yet. This repository is available for developers to inspect and build under Apache-2.0.

## What you can do

| Workflow | In the app |
| --- | --- |
| Capture and inspect | Discover local runtimes, capture HTTP(S), search sessions, inspect headers, bodies, timing, and errors. |
| Replay and organize | Edit captured requests, use collections and environments, and keep secret values in macOS Keychain. |
| Mock and debug | Return fixtures, inject latency or failures, and pause requests or responses at breakpoints. |
| Understand app context | Add optional Swift or Kotlin SDK context, including screen, feature, source, and logs. |
| Compare sessions | Find missing calls, payload and schema changes, retries, errors, and timing differences. |
| Explain with AI | Preview locally redacted evidence before explicitly sending it to an optional provider. |

Traffic and workspace data stay on your Mac by default. The optional AI flow makes an external request only after you preview the context and choose to send it. See [Security and privacy](docs/SECURITY_AND_PRIVACY.md) for the implemented boundaries and the validation still pending.

## Build from source

The desktop target is **macOS**. This repository was most recently built on Apple Silicon with macOS 27; other macOS versions and Intel builds have not been validated yet.

Install:

- Xcode Command Line Tools and a Rust toolchain compatible with `rust-version = 1.85`;
- Node.js 20.19+ or 22.12+ and pnpm 10.15.0;
- `mitmdump` from mitmproxy for capture;
- Xcode and an iOS Simulator runtime for iOS work, or Android SDK Platform Tools and an Android Emulator for Android work.

Then, from the repository root:

```sh
pnpm install --frozen-lockfile
pnpm tauri dev
```

To build a local macOS app bundle:

```sh
pnpm tauri build --bundles app
```

The bundle appears at `target/release/bundle/macos/Mobile API Studio.app`. It is ad hoc signed for local preview. Read [macOS release preparation](docs/MACOS_RELEASE.md) before distributing a build. `mitmdump` is installed separately; the app packages its Python capture bridge and can discover a standard Homebrew install or use the absolute path set in Settings.

## How capture works

1. Boot an iOS Simulator or Android Emulator and open **Connect**.
2. Run **Connection Doctor**, select the runtime, and start a session.
3. Follow the runtime's proxy and development CA guidance, then use **Traffic**, **Replay**, **Mocks**, and **Compare**.

The iOS Simulator proxy is configured manually. Android Emulator proxy changes are journaled for rollback. Apps with certificate pinning need their own debug configuration; Mobile API Studio does not bypass pinning. Optional [iOS and Android SDKs](docs/SDK_INTEGRATION.md) add app context without requiring production instrumentation.

## For contributors

The desktop app uses Tauri 2, React, TypeScript, Rust, SQLite, and mitmproxy. The iOS SDK is a Swift Package; the Android SDK is a Kotlin library with an OkHttp interceptor. Browse the [architecture](docs/ARCHITECTURE.md), [roadmap](docs/ROADMAP.md), and [contribution guide](CONTRIBUTING.md) to find a starting point. Report vulnerabilities through the [security policy](SECURITY.md).

Implementation Phases 0–5 were merged without automated tests or CI as phase gates. `cargo test --workspace` currently compiles the workspace but contains no automated Rust tests. The repository owner controls the separate [final validation](docs/FINAL_VALIDATION.md) and future test strategy; please describe what you actually verified in a pull request.

## Support the project

[![Animated Buy me a coffee card linking to DagerottDev's support page](.github/assets/buy-me-a-coffee.gif)](https://buymeacoffee.com/dagerottdev)

- [Buy Me a Coffee](https://buymeacoffee.com/dagerottdev) — international support.
- [Buy DagerottDev a Chai on Bondin](https://bondin.io/dagerottdev) — support from India.

Support is optional. Contributions, bug reports, and documentation improvements are welcome too.

## License

Mobile API Studio is licensed under the [Apache License 2.0](LICENSE). Third-party dependencies retain their own licenses.
