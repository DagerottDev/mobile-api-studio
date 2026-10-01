# Proxy rules — milestone 2

This change builds on capture targets. It is a macOS source implementation with focused loopback checks; physical-device, Simulator/Emulator, compatibility, and release acceptance remain owner-led. Use disposable data and separate ports when validating it.

## Rules and matching

The Mocks page now has an independent proxy rule editor and a combined ordering view. Existing mocks remain editable below it. Rules sort by priority, creation timestamp, and ID. Host matching is case insensitive. Exact, wildcard (`*`, `?`), and full-string Rust regex matching share the same implementation as the preview and capture matcher. Patterns are bounded to 256 bytes; regex lookaround/backreferences are unsupported.

Connection policy runs before HTTP. Ordered request rewrites and breakpoints run next, then the first terminal Allow, Block, Map Local, Map Remote, or legacy mock. Response rewrites and breakpoints run after the mock response processing. Request breakpoint edits re-match terminal and response rules without executing request mutations twice. Existing mocks retain their own first-match order.

| Action | Behavior |
| --- | --- |
| Allow / Block | Allow ordinary routing or return a configured 400–599 response. These are terminal actions and compete with mocks in the shared order. |
| Map Local | Serve a copied regular file from the private `proxy-maps` directory, at most 2 MiB. Absolute paths, traversal and symlinks are rejected. |
| Map Remote | Replace the request URL with a bounded, credential-free HTTP(S) URL. |
| Request / response rewrite | Set/remove up to 64 headers; optionally replace the body with at most 2 MiB of UTF-8 text. Audit records contain header operations and byte counts, not secret values. |
| Breakpoint | Pause at the selected stage using the existing decision UI. Invalid decisions stop the affected flow. Proxy breakpoint expiry stops the flow; legacy mock timeout behavior remains unchanged. |
| No cache | Send `Cache-Control: no-cache`, then return `Cache-Control: no-store` without `Expires`. |
| Block cookies | Remove request `Cookie` and response `Set-Cookie`. |
| HTTPS inspection | First matching connection host rule chooses inspection or encrypted TCP passthrough before TLS. Requires method `TLS` and wildcard path. |
| DNS override | Return an A/AAAA answer from a configured IP address for matching queries sent to the DNS listener. Requires method `DNS` and wildcard path. Other names use mitmproxy's resolver. |

Changes apply to new requests and TLS connections. Reconnect clients after changing TLS policy: an established connection retains its inspection decision. Bypassed TLS has no decrypted HTTP flow to inspect. UDP/QUIC passthrough is currently rejected with a diagnostic; this change does not claim HTTP/3 support. Explicit proxy modes match the connection destination; local capture uses SNI when present, otherwise the destination address. See mitmproxy's [TLS hook contract](https://docs.mitmproxy.org/stable/api/mitmproxy/tls.html).

Completed HTTP flows store applied rule IDs and up to 32 redacted change records, shown in Traffic. DNS answers and encrypted passthrough are not yet listed as HTTP flows. Rule execution errors appear in the editor's recent diagnostics. An unavailable or malformed matcher response stops the affected flow/handshake instead of silently skipping rules.

## Listener modes

Connect offers reverse proxy, upstream proxy, SOCKS5, and DNS listeners using [mitmproxy's native modes](https://docs.mitmproxy.org/stable/concepts/modes/). They bind to loopback and require explicit client configuration. They do not change host system proxy or DNS settings. Reverse/upstream targets accept HTTP(S) host URLs without credentials, path, query, or fragment. Default ports are 8185 for proxy listeners and 8186 for DNS.

The capture engine waits up to 20 seconds for the addon `running` signal, which follows listener setup. An exited child or readiness timeout fails startup. Later process exits are monitored and shown as “Capture stopped”; disconnect restores any session routing before reconnection. Lazy upstream connection strategy lets pre-TLS policy and local maps run before upstream TLS negotiation. HTTP/2 client negotiation was checked with this setting.

## Storage and export

Database migration 5 adds ordered rule storage; the existing migration backup runs before upgrading. Rule definitions are limited to 1,000 rules and 16 MiB total. The private matcher socket has an owner-only directory/socket, bounded requests, at most 64 concurrent connections, timeouts, and an 8 MiB matching-rule response ceiling. Oversized matching sets stop the flow with a diagnostic.

Portable bundle version 4 carries rules. Versions 2 and 3 remain importable. Every imported rule starts disabled. Map Local files, sensitive action definitions, and flow rule audit metadata are omitted from exports. Omitted definitions are represented by disabled placeholders and counted in the import result. Replace imports insert/delete rules inside the workspace transaction. The Settings page previews the exact bundle before download; captured bodies remain included and can contain application data.

## Checks and remaining acceptance

Run focused checks from this worktree:

```sh
cargo test -p app-core
cargo test -p capture-mitm
cargo test -p core-model -p storage proxy_rule
python3 scripts/check-proxy-rules.py
pnpm build
```

The live loopback check uses installed mitmdump, temporary origin/CA material, an isolated rule socket, and disposable ports. It checks TLS inspection, ordered passthrough, restoration after disabling rules, malformed matcher fail-closed behavior, HTTP/2 client negotiation when curl supports it, and terminal re-matching after both proxy and mock breakpoint edits. This is focused regression evidence, not device or release certification.

Owner-led acceptance still includes:

1. Simulator/Emulator HTTP/1.1 and HTTP/2 rule interactions, replay, SDK correlation, AI redaction, and import/export regression.
2. Map Local/Remote, rewrites, cookie/cache removal, conflicting rules, and request/response breakpoint behavior with real app traffic.
3. Reverse/upstream/SOCKS5 routing and DNS A/AAAA override/fallback using clients configured for those modes.
4. Physical-device and Mac process capture, disconnect, interrupted-start recovery, and capture engine termination.
5. Export preview contents and disabled imported rule review.

Milestones 3–8 (protocol inspection, network conditions, compose/interchange, scripting/automation, hosted collaboration, and Windows/Linux) remain part of the expansion roadmap.
