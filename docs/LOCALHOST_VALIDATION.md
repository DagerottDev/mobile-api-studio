# Localhost migration validation record

Date: 2026-09-26. This record covers the source-built localhost implementation on the development Mac. It does not close the owner-led device checklist in [FINAL_VALIDATION.md](FINAL_VALIDATION.md).

## Verified locally

- `cargo check --workspace --locked --offline` compiled the Rust workspace, including the historical Tauri crate and new `app-core` and Axum service.
- `cargo test -p mobile-api-studio-server --offline` passed four focused tests for Host/Origin, token, security headers, and token generation. The original 71 Tauri command names match all 71 entries in the new dispatch allowlist.
- `npm run build` in `apps/desktop` passed TypeScript and Vite production build. `./scripts/run-local.sh --port 18180 --no-open --data-dir /private/tmp/mas-local-smoke-20260924` built the UI and launched the native service.
- `lsof` showed the UI listener on `127.0.0.1:18180`. A second service using the same data directory was rejected by the process lock. The lock was removed on normal shutdown; a stale test lock was recovered on restart.
- Allowed `health` returned HTTP 200; an unknown command returned structured HTTP 404. Unexpected Host and Origin values returned HTTP 403; a missing command token returned HTTP 401. Rejected Host responses included the security headers.
- The Codex in-app browser rendered all nine local routes. Direct requests to each route returned HTTP 200; unknown API paths returned structured HTTP 404. Browser back and refresh preserved the route. Tab focus reached the navigation and Return activated Traffic. The Traffic list and inspector changed from two columns at 1440 px to one at 520 px. All nine routes were checked for page-level horizontal overflow at 520 px and 375 px; none overflowed. The dark UI and empty states were visually inspected.
- A synthetic onboarding step persisted across service restart. A synthetic session imported into a disposable database appeared in Workspace and remained visible after refresh.
- A synthetic import bundle containing a secret environment value and fake Keychain reference reported one omitted secret. The subsequent export contained neither value nor reference. The Settings browser export produced a download confirmation.

## Still required for release

- Owner-led iOS Simulator and Android Emulator capture, certificate trust, Replay, SDK correlation, mocking, breakpoints, and proxy rollback after normal and forced stops.
- Real two-tab connection races with a booted runtime and an existing-data migration using a backup of the owner's application data.
- Visual and keyboard checks in macOS light appearance and the full narrow/wide UI across the nine areas.
- Optional AI preview and send with a configured provider. The code retains the preview fingerprint check, but no external request was sent during this pass.

Android Platform Tools were not on this Mac's `PATH` during Connection Doctor. `simctl` discovered installed Simulator definitions, but none was booted for this pass. Keep the localhost release labeled as a source-build preview until the remaining checks are recorded.
