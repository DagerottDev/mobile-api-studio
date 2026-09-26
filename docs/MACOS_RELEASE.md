# macOS localhost release preparation

The first distribution method is a source build. No prebuilt, signed, or notarized Mac executable is published. Apple Developer ID signing and notarization are not required to run the source-built localhost service; the developer still needs the macOS, Rust, Node, mitmproxy, and mobile runtime tooling below. The earlier Tauri app bundle was a local preview and is historical. See the [local validation record](LOCALHOST_VALIDATION.md) for checks completed so far.

## Build and run

Install Xcode Command Line Tools, Rust, Node.js 20.19+ or 22.12+, pnpm 10.15.0, and `mitmdump`. Install Xcode and Simulator runtimes for iOS work, or Android SDK Platform Tools and an Emulator for Android work. Then, from the repository root:

```sh
pnpm install --frozen-lockfile
./scripts/run-local.sh
```

`./scripts/run-local.sh` builds the browser UI, compiles the Rust service, binds `127.0.0.1:8180`, and opens the URL. `./scripts/run-local.sh --port 8190` changes only the UI port; capture and SDK ingestion use `8181` and `8182`. The capture bridge runs from this source tree. Keep the source tree in place while using the service. If `mitmdump` is not on `PATH`, set its absolute path in Settings.

Close the old desktop app before using the shared directory at `~/Library/Application Support/dev.mobileapistudio.desktop`. Back up `app.db` before any future schema migration. Stop the service with Ctrl+C to end capture and restore the Android proxy. After a forced stop, reopen Connect and use the pending rollback recovery control before another capture.

## Release decision gates

- Complete the [owner-led validation checklist](FINAL_VALIDATION.md) on an iOS Simulator and Android Emulator. Record the OS, CPU, tool versions, runtimes, and result for each gate.
- Verify existing-data restart, workspace import/export and omitted secrets, AI preview before send, two-tab connection races, local API rejection paths, and normal and forced-stop proxy recovery.
- Check light and dark appearances, narrow and wide windows, keyboard use, and all nine stable URLs.
- Review third-party licenses and how contributors install `mitmdump`.
- Publish source instructions only after those checks pass. If a packaged Mac distribution is later desired, plan Developer ID signing, notarization, and clean-Mac verification separately.

The Android Emulator reaches the host capture listener through `10.0.2.2`; the UI service remains on `127.0.0.1`. This is a same-Mac tool and is not hosted on the internet.
