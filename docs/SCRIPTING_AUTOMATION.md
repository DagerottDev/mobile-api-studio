# Scripting and local automation

Script hooks are optional ordered proxy rules. New rules and imported scripts are disabled until the user enables them. Request hooks run with request rewrites; response hooks run after the upstream response; WebSocket hooks run on each matched message. Terminal rules retain their existing precedence. A script failure stops its flow and records `script_hook_failed` in the flow and rule diagnostics.

## Script contract

Define a synchronous `function transform(event) { return event; }`. Return the complete event object. Request events contain `stage`, `method`, `url`, `headers`, and `body`; response events additionally contain `statusCode`. Headers are `{name,value,sensitive}` records. Bodies contain `dataBase64`, `contentType`, and `isTruncated:false`; bytes are the raw payload, including any content encoding. If the body is unchanged, the adapter preserves its encoding metadata. Changed bodies use the existing breakpoint validation and normalization.

WebSocket events add `opcode`, `fromClient`, and `dropped`. Only body bytes and the dropped flag are writable; a text frame must remain valid UTF-8. Set `dropped=true` to discard the current message. Response hooks cannot change request identity. Invalid URLs, headers, body encodings, immutable fields, asynchronous returns, or worker failures stop traffic rather than forwarding an unchecked result.

```javascript
function transform(event) {
  event.headers.push({ name: "X-Debug", value: "local", sensitive: false });
  return event;
}
```

The dedicated QuickJS worker has no host callbacks, module loader, filesystem, network, or OS APIs. Each invocation starts a disposable process with an empty environment. It is process isolation with a bounded JavaScript runtime, not an OS sandbox. The executable must be installed beside the local service; `scripts/run-local.sh` builds both.

Bounds: script 64 KiB; stdin 3 MiB; serialized event/output 2 MiB; engine heap 32 MiB; stack 256 KiB; worker execution 100 ms; parent exchange one second. At most four workers run concurrently, and each request, response, or individual WebSocket-message stage permits sixteen hooks within two seconds including queue time. Base64 and JSON overhead mean a body below the ordinary capture limit can still exceed the script event limit. Failure messages omit script source and payloads.

General workspace export omits script source. In Mocks, select a script rule and prepare its explicit export preview, review any secrets typed in source, then download the version-7 bundle. Import through Settings; rules remain disabled. Older bundles cannot introduce executable hooks.

## CLI and MCP

On macOS/Linux the service creates `<data-dir>/control/socket`, with a user-owned mode-0700 parent and mode-0600 socket. It checks kernel-reported peer UID, rejects symlinks and an existing live listener, bounds requests/responses, and removes its own socket on orderly shutdown. This private channel does not use the browser token or listen on the LAN.

```sh
printf '{}' | python3 scripts/mas-cli.py --socket '/your/data-dir/control/socket' health
python3 scripts/mas-cli.py --socket '/your/data-dir/control/socket' --mcp
```

The optional MCP bridge uses JSON-RPC over stdio with initialization and a single allowlisted tool. Available operations cover capture targets, sessions/flows/search, proxy rules, network profiles, and selected interchange/workspace export. Requests are capped at 128 KiB and responses at 16 MiB; arbitrary commands, AI/secret-store access and direct replay are excluded. Export output appears in the CLI or MCP tool result for review; it is not uploaded automatically.

## Verification and acceptance

Run `cargo test -p app-core -p mobile-api-studio-script-worker --offline --locked`, `cargo test -p mobile-api-studio-server control_socket --offline --locked`, `python3 scripts/check-mas-cli.py`, the frontend production build, and `python3 scripts/check-script-hooks.py` after building the service and worker. The live hook check uses disposable loopback data and never changes host proxy settings.

Current core checks pass, including disabled script import, rejection of scripts in older bundles, and explicit selected export. The worker limit check, four private-socket checks, CLI/MCP self-check and real service/MCP lifecycle check pass. The real loopback hook check passes request header/body edits, response status/body edits, 131,073-byte output, infinite-loop traffic stop with both queue and persisted flow diagnostics, WebSocket text/binary transforms and drop, and the sixteen-hook stage cap. The network-condition regression also passes after the shared HTTP framing fix. At that source-verification checkpoint, browser preview automation was blocked by tool security policy. Subsequent owner-authorized Mac computer use passed bounded script/mock, Compose, Compare and network workflows; portable package and device checks are recorded in [PLATFORM_ACCEPTANCE.md](PLATFORM_ACCEPTANCE.md). Those later cases do not establish full UI, physical-device or release acceptance.
