# Mobile API Studio

**A local-first API debugger for iOS Simulators and Android Emulators.** Capture a request, inspect what happened, replay it, mock its response, and compare two sessions in a browser on the same Mac.

[Build from source](#build-from-source) · [How capture works](#how-capture-works) · [Contribute](CONTRIBUTING.md) · [Support the project](#support-the-project)

> **Release status:** The localhost service and browser UI are implemented for source builds. See the [local validation record](docs/LOCALHOST_VALIDATION.md) for checks completed so far. The owner-led iOS Simulator, Android Emulator, migration, and recovery checks in the [final validation checklist](docs/FINAL_VALIDATION.md) remain open. Do not describe this as a validated device release yet. The earlier Tauri macOS app build is historical; no signed or notarized download is offered.

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

The first source-built target is **macOS**. The browser UI and Rust service run on the same Mac at `http://127.0.0.1:8180`. Other macOS versions and Intel builds have not been validated yet.

Install:

- Xcode Command Line Tools and a Rust toolchain compatible with the workspace `rust-version`;
- Node.js 20.19+ or 22.12+ and pnpm 10.15.0;
- `mitmdump` from mitmproxy for capture;
- Xcode and an iOS Simulator runtime for iOS work, or Android SDK Platform Tools and an Android Emulator for Android work.

Then, from the repository root:

```sh
pnpm install --frozen-lockfile
./scripts/run-local.sh
```

The command builds the React UI, starts the Rust service, and opens the local URL. Pass `--port 8190` to use another UI port. Capture and SDK ingestion remain on `8181` and `8182`. Stop the service with Ctrl+C so it can end capture and restore an Android proxy. Keep the earlier desktop app closed while using the same data directory.

Workspace data stays in `~/Library/Application Support/dev.mobileapistudio.desktop`. Back up `app.db` before any future schema migration. Browser import uses a selected JSON file; export downloads a redacted workspace bundle. The service listens only on `127.0.0.1`, checks Host and Origin, and requires a process-lifetime token for commands. The token is held in browser memory, outside URLs and logs.

`mitmdump` is installed separately. The service uses the Python bridge in this repository and can discover a standard Homebrew install or use the absolute path set in Settings. Read [macOS release preparation](docs/MACOS_RELEASE.md) for the validation gates.

## How capture works

1. Boot an iOS Simulator or Android Emulator and open **Connect**.
2. Review the readiness workbench in **Connect**, select the runtime, and start a session.
3. Follow the runtime's proxy and development CA guidance, then use **Traffic**, **Replay**, **Mocks**, and **Compare**.

The iOS Simulator proxy is configured manually. Android Emulator proxy changes are journaled for rollback. Apps with certificate pinning need their own debug configuration; Mobile API Studio does not bypass pinning. Optional [iOS and Android SDKs](docs/SDK_INTEGRATION.md) add app context without requiring production instrumentation.

## For contributors

The current app uses Axum, React, TypeScript, Rust, SQLite, and mitmproxy. The iOS SDK is a Swift Package; the Android SDK is a Kotlin library with an OkHttp interceptor. The old Tauri source remains temporarily for parity comparison while device validation is pending. Browse the [architecture](docs/ARCHITECTURE.md), [roadmap](docs/ROADMAP.md), and [contribution guide](CONTRIBUTING.md) to find a starting point. Report vulnerabilities through the [security policy](SECURITY.md).

Implementation Phases 0–5 were merged without automated tests or CI as phase gates. The localhost migration adds focused API security tests and manual browser smoke checks; no CI gate has been added. The repository owner controls the separate [final validation](docs/FINAL_VALIDATION.md); please describe what you actually verified in a pull request.

## Support the project

[![Animated Buy me a coffee card linking to DagerottDev's support page](.github/assets/buy-me-a-coffee.gif)](https://buymeacoffee.com/dagerottdev)

- [Buy Me a Coffee](https://buymeacoffee.com/dagerottdev) — international support.
- [Buy DagerottDev a Chai on Bondin](https://bondin.io/dagerottdev) — support from India.

Support is optional. Contributions, bug reports, and documentation improvements are welcome too.

## License

Mobile API Studio is licensed under the [Apache License 2.0](LICENSE). Third-party dependencies retain their own licenses.
