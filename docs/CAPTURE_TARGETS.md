# Capture targets — milestone 1

This is a macOS-first source implementation. Device and release validation remain owner-led. Keep the localhost release's data directory and ports separate when checking this milestone; use a disposable `--data-dir` and UI port.

## Targets and connection behavior

- **iOS Simulator and Android Emulator:** The existing `connect_device` command and rollback behavior remain available. `connect_capture_target` routes these target types through that command and verifies the selected runtime is still discoverable.
- **This Mac or one process:** Uses mitmproxy local mode, without changing the Mac system proxy. Select a running PID to limit interception. macOS permission and HTTPS trust prompts depend on the machine and app.
- **Physical iOS or Android:** Select a private LAN interface and enter one device IPv4 address. The app starts a proxy listener on that interface at port `8183`; it accepts TCP connections only from that address and forwards them to the loopback mitmdump listener at `8181`. Set and remove the device's Wi-Fi HTTP proxy manually. The app does not alter device settings, bypass certificate pinning, or expose its browser command API on the LAN.

For HTTPS, install the development capture CA using `mitm.it` through the proxy. On iOS, enable full trust for that CA in Certificate Trust Settings. On Android, configure the debug app to trust user CAs. Use only developer-controlled devices.

Physical-device SDK telemetry has a separate listener on the selected LAN interface at port `8184`. It accepts only the paired address and requires the session's `X-MAS-Pairing-Token`; configure the SDK's host/port or base URL and pairing token explicitly. The loopback SDK listener remains on `8182`. The pairing token is shown only at connection time and is not stored in sessions or exports. It remains valid until disconnect. The LAN SDK path currently uses HTTP, so use it only on a trusted development network and keep secrets out of telemetry.

Use the host and token shown in Connect. For an iOS debug build, set `MobileAPIStudioConfiguration(desktopBaseURL: URL(string: "http://<Mac LAN IP>:8184")!, pairingToken: "<token>")`. For Android, set `MobileAPIStudioConfiguration(desktopHost = "<Mac LAN IP>", desktopPort = 8184, pairingToken = "<token>", enabled = true)`. Do not put the token in source control.

Disconnect closes the LAN listeners and capture engine. If the app exits unexpectedly, its listeners disappear and it has no Mac or physical-device setting to restore. The existing Android Emulator proxy rollback journal remains independent.

## Data compatibility

Session metadata records a versioned capture target and mode. Existing sessions have neither field. Database migration 4 creates both columns and makes a SQLite backup before changing an existing database. New workspace exports use bundle version 3; version 2 bundles remain importable.

## Owner-led acceptance checklist

- Existing Simulator and Emulator connect, capture, disconnect, and interrupted-start recovery still work.
- Mac-wide capture records HTTP and HTTPS from a development app; selecting one PID captures it without unrelated processes.
- One physical iOS or Android development device captures HTTP and HTTPS after manual proxy and CA setup. An unpaired address cannot connect to either LAN listener.
- A paired physical app sends SDK telemetry only with the token; wrong or absent tokens fail. Disconnect closes both device-facing listeners.
- Reopen old sessions and import an older workspace bundle. Verify target and mode metadata survive restart and export/import.
- Confirm the browser API and loopback SDK listener are unavailable from the LAN, and check that no CA private material or pairing token appears in an export.

These checks have not been claimed as completed by the source implementation.
