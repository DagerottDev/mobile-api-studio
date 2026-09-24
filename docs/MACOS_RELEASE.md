# macOS release preparation

This project can produce a macOS app bundle. Distribution remains an owner decision after the final validation checklist in [FINAL_VALIDATION.md](FINAL_VALIDATION.md). The currently checked build is an Apple Silicon local preview, not a notarized public release.

## Prerequisites

- macOS with Xcode Command Line Tools, Rust, Node.js, and pnpm 10.15.0.
- Install workspace dependencies with `pnpm install --frozen-lockfile` and build Rust with the committed `Cargo.lock`.
- Install `mitmdump` separately or set its absolute executable path in Settings. The Python capture bridge is packaged inside the app; `mitmdump` itself is not bundled.
- Install Xcode and the intended iOS Simulator runtime for iOS capture, or Android SDK Platform Tools and an emulator for Android capture.

Finder-launched apps may have a shorter `PATH` than a terminal. If Connection Doctor cannot find `mitmdump`, set its absolute path in Settings and restart. Android Platform Tools must be on the app's process `PATH` for Android discovery.

## Build and inspect

From `apps/desktop`:

```sh
npm run build
./node_modules/.bin/tauri build --bundles app,dmg
```

The app is written to `target/release/bundle/macos/Mobile API Studio.app` and the disk image to `target/release/bundle/dmg/`. Verify the app contains `Contents/Resources/sidecars/mitm-addon/mas_bridge.py`. Run `codesign --verify --deep --strict --verbose=2` on the app and `hdiutil verify` on the disk image. Launch the packaged app and run Connection Doctor; a source-tree launch does not validate the packaged resource path.

The default signing identity is ad hoc (`-`) for local preview builds. Ad hoc signing is not suitable for frictionless distribution to other Macs. For direct distribution, install an Apple Developer ID Application certificate, set `APPLE_SIGNING_IDENTITY` to that identity, provide notarization credentials, and rebuild. Follow [Tauri's macOS signing and notarization guide](https://v2.tauri.app/distribute/sign/macos/). Do not publish the ad hoc artifact as a finished macOS release.

## Release decision gates

- Choose the public version, product name, and supported macOS/CPU matrix. The source license is Apache-2.0; the current app identifies itself as `0.0.1` and the UI says `pre-alpha`.
- Complete the relevant workflows in [FINAL_VALIDATION.md](FINAL_VALIDATION.md), especially iOS/Android capture, Android proxy rollback, import/export, secrets, and AI redaction. A build and launch check does not cover them.
- Review third-party licenses and how `mitmdump` will be installed or managed on recipient Macs.
- Sign with Developer ID, notarize, staple, and verify the exact artifact intended for distribution. Verify it on a clean Mac and record its SHA-256 digest.

For Android, the emulator's `10.0.2.2` address reaches the host loopback interface, so the capture listener binds to `127.0.0.1` rather than exposing a proxy on every network interface. See [Android's emulator address documentation](https://developer.android.com/studio/run/emulator-networking-address).
