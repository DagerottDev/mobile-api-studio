# Research Notes

These references influenced the initial architecture. Re-verify behavior against the installed toolchain while implementing.

## Tauri 2

Tauri 2 supports a web frontend with Rust application logic and targets desktop platforms. This makes it a good fit for a React/TypeScript inspector UI with a Rust system-integration core.

- https://tauri.app/
- https://v2.tauri.app/start/

## mitmproxy

mitmproxy provides TLS-capable interception and multiple capture modes. Its regular proxy mode is the simplest baseline, while local capture can target processes on the same host on supported operating systems. Its certificate documentation explicitly covers the CA trust requirement and certificate-pinning limitation.

- https://docs.mitmproxy.org/stable/concepts/modes/
- https://docs.mitmproxy.org/stable/concepts/certificates/
- https://github.com/mitmproxy/mitmproxy

Initial design recommendation: use mitmdump as an implementation detail behind `CaptureEngine`, not as the product architecture itself.

## Android Emulator proxying

Android's official emulator documentation distinguishes the Android System Proxy used for app debugging from the Emulator Proxy used primarily for network/firewall routing. HTTPS inspection through a debugging proxy requires certificate trust, and some apps may ignore system proxy settings.

- https://developer.android.com/studio/run/emulator-networking-proxy
- https://developer.android.com/reference/android/provider/Settings.Global

Implementation consequence: connection must be capability-based and diagnostics must identify apps that bypass proxy/trust settings.

## iOS Simulator / simctl

Apple documents `simctl` as the command-line interface for Simulator workflows and demonstrates adding a CA certificate to the trusted root store with:

```text
xcrun simctl keychain booted add-root-cert myCA.pem
```

- https://developer.apple.com/videos/play/wwdc2020/10647/

Implementation consequence: CA installation can be automated for selected simulators, while traffic routing still needs its own connection strategy and rollback.

## Pulse

Pulse is an open-source Swift logging system that records URLSession traffic and can provide app-integrated network/log inspection. It explicitly positions itself as app-side instrumentation rather than a network proxy.

- https://github.com/kean/Pulse

Implementation consequence: v0.4 SDK mode is a valid complement to proxy mode.

## Chucker

Chucker is an open-source Android/OkHttp inspector implemented as an OkHttp interceptor.

- https://github.com/ChuckerTeam/chucker

Implementation consequence: Android SDK mode should integrate naturally as an interceptor and provide a no-op/release-safe setup pattern.

## Architectural conclusion

The strongest product is hybrid:

```text
Proxy mode -> broad capture without app integration
SDK mode   -> app-owned context and networking stacks proxy mode cannot inspect reliably
```

These modes should converge on the same normalized Flow model and UI.
