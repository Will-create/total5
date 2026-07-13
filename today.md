# Today: Native Total.rs Web Server

Date: 2026-07-12  
Status: In progress  
Sole goal: Replace the Axum runtime with a framework-owned HTTP/HTTPS server modeled on `../framework5/http.js`, `routing.js`, `controller.js`, and `websocket.js`.

This file is the live progress ledger for the migration. Update it after every completed milestone, newly discovered blocker, or change to a verification result. A checked item means the behavior exists and has been verified; source structure or similarly named types alone are not completion.

## Current implementation truth

- The live ordinary HTTP application server now runs through the framework-owned Tokio TCP transport; `Total::run_on()` no longer calls `axum::serve`.
- Axum, Tower, and `tower-http` have been removed from source, manifests, and lockfiles. The native dispatcher is the only HTTP routing path.
- `src/server.rs` contains a framework-owned, incremental HTTP/1.0 and HTTP/1.1 request reader, response writer, buffered connection loop, and shutdown-aware TCP accept loop.
- Shutdown now stops accepting connections, closes idle keep-alive connections, drains in-flight responses, and aborts only connections that exceed the configurable drain timeout.
- HTTPS now loads PEM certificate chains and private keys, performs timeout-bounded Rustls handshakes, and passes decrypted streams to the same native connection loop and dispatcher as HTTP.
- Request handlers have a configurable five-second default deadline. Timed-out futures are dropped (canceled) and receive a Total.js-compatible 503 response.
- Configuration now implements the Total.js default key set, `.env`/base/mode/plugin/version load chain, typed values, encoded values, generated-value persistence, special transformations, directory overrides, global snapshots, and runtime reload hooks. JavaScript execution through `(Eval)` is intentionally replaced by JSON parsing.
- The request reader currently supports strict header/body limits, `Content-Length` bodies, malformed-request detection, and incomplete connection/body detection.
- Chunked transfer decoding supports chunk extensions, trailers, decoded-body limits, and preservation of bytes belonging to the next request. Unsupported transfer codings and Content-Length/Transfer-Encoding conflicts are rejected.
- The native transport is connected to a framework-owned dispatcher for REST/API routing, parameters, AUTH, middleware, controller contexts, actions, and static-file fallback.
- WebSocket routes now use a framework-owned RFC 6455 upgrade handshake and frame pump. Text/binary input, outbound text, fragmentation, ping/pong, close, masking, UTF-8, opcode, control-frame, and size validation are native. Compression, subprotocols, richer route context, and outbound clients remain.
- The architecture and parity roadmap is recorded in `README.md`.

## Verified baseline

Verified on 2026-07-12:

- [x] `cargo test` passes: 47 passed, 0 failed.
- [x] Eight native HTTP parser/writer/connection unit tests pass.
- [x] A raw HTTP duplex integration test proves native route matching, parameter extraction, middleware, controller creation, and response serialization without Axum routing.
- [x] `cargo build --manifest-path testapp/Cargo.toml` passes.
- [x] The test application continues to build against the current framework.

These results exercise the native dispatcher; no alternate application-framework router remains.

## Progress

### Completed

- [x] Document the native-server architecture, restrictions, parity phases, and acceptance standard in `README.md`.
- [x] Add low-level incremental HTTP/1.0 and HTTP/1.1 request parsing.
- [x] Enforce request header and body size limits.
- [x] Read `Content-Length` request bodies.
- [x] Detect malformed request lines, malformed headers, invalid lengths, incomplete headers, and incomplete bodies.
- [x] Serialize `http::Response<Bytes>` values with status, headers, body, and an inferred Content-Length.
- [x] Add focused parser, body-limit, and response-serialization tests.
- [x] Preserve over-read bytes across keep-alive and pipelined requests.
- [x] Add HTTP/1.1 default keep-alive, `Connection: close`, and HTTP/1.0 close/keep-alive response behavior.
- [x] Add configurable connection read/write timeouts and protocol-error responses.
- [x] Add a Tokio TCP accept loop with a shutdown signal and per-connection tasks.
- [x] Connect `Total::run_on()` to the native transport and dispatcher.
- [x] Dispatch REST/API routes, parameters, AUTH, middleware, actions, controller contexts, and static fallback on the native live path.
- [x] Convert framework `Response` values directly into `http::Response<Bytes>`.
- [x] Implement native HEAD body suppression and OPTIONS Allow responses.
- [x] Remove Axum, Tower, `tower-http`, and the now-unused `futures-util` dependency from source, manifests, and lockfiles.
- [x] Migrate the former in-process router tests to the native dispatcher.
- [x] Decode chunked request bodies with extensions/trailers, decoded-body limits, malformed-input checks, and pipelined-byte preservation.
- [x] Implement the RFC 6455 accept handshake without a WebSocket dependency.
- [x] Attach native WebSocket upgrades to the buffered HTTP connection without losing over-read frame bytes.
- [x] Implement masked client frame decoding, outbound text framing, fragmentation, ping/pong, close, and protocol/size validation.
- [x] Add a raw-byte WebSocket handshake and echo test using a masked client frame.
- [x] Gracefully drain active connections on shutdown with a configurable timeout.
- [x] Add Rustls HTTPS with PEM loading, handshake timeouts, graceful draining, and the shared native dispatcher.
- [x] Add configurable handler timeouts and cancel timed-out handler futures.

### Active milestone

- [ ] Complete native HTTP behavior and migrate the remaining Axum-backed features/tests.

The next implementation slice is response compression and byte ranges. WebSocket compression, subprotocol negotiation, route context parity, and the outbound client remain later WebSocket work.

### Not started

- [ ] Compression and byte ranges.
- [ ] Multipart uploads.
- [ ] Cookies and CORS.
- [ ] Blacklist, throttling, pending-request tracking, request limits, and statistics.
- [ ] WebSocket compression, subprotocols, route context parity, and outbound client support.
- [ ] Raw TCP/TLS end-to-end test migration.

## Work plan

1. Build the TCP connection lifecycle: accept loop, incremental buffered requests, keep-alive, timeouts, disconnect handling, protocol errors, and graceful shutdown.
2. Replace Axum route storage and lookup with framework-owned matching for REST/API routes, parameters, AUTH, middleware, controller creation, static fallback, HEAD, and OPTIONS.
3. Convert framework `Response` values directly into `http::Response<Bytes>` and connect the test application to the native server.
4. Add chunked request decoding, compression, byte ranges, multipart uploads, cookies, CORS, request limits, blacklist/throttling, pending requests, and statistics.
5. Add Rustls HTTPS using the same dispatcher and implement framework-owned WebSocket upgrades and framing.
6. Migrate all server tests to raw HTTP/TCP/TLS tests, verify the test application, then remove Axum, tower, and tower-http from the manifest and lockfiles.

## Blockers and risks

- **WebSocket parity gap:** Basic native serving works, but compression, subprotocols, route params/AUTH parity, connection collections, and the outbound client are incomplete.
- **Socket-test constraint:** This workspace blocks loopback bind with `Operation not permitted`; native dispatch is verified with raw HTTP bytes over `tokio::io::duplex`. A true loopback TCP fixture remains an acceptance gate for an environment that permits sockets.
- **Parity risk:** Existing end-to-end tests can remain green while exercising Axum. Native-path tests must prove which dispatcher and transport served each request.
- **Scope risk:** HTTP, HTTPS, and WebSocket transports must converge on one dispatcher; separate behavior paths would make Total.js parity unreliable.

## Acceptance gates

- [x] No Axum, Tower, or tower-http dependency or executable source reference remains.
- [x] HTTP and HTTPS use one Total.rs dispatcher.
- [ ] REST routes, API routes, parameters, AUTH, middleware, schemas/actions, static files, and WebSockets work in the test application through the native server.
- [ ] Raw TCP tests cover HTTP/1.0, HTTP/1.1, keep-alive, malformed input, limits, chunked bodies, HEAD/OPTIONS, uploads, ranges, and shutdown.
- [ ] TLS tests cover valid serving and invalid certificate/configuration failures.
- [ ] Focused behavior fixtures match the local Node.js Total.js implementation.
- [ ] `cargo fmt --check` passes.
- [x] `cargo test` passes without application-framework-backed server tests.
- [x] `cargo build --manifest-path testapp/Cargo.toml` passes against the native server.
- [ ] Final diff and dependency checks show no forbidden framework remnants.

## Architecture constraints

- `../framework5` is the authoritative behavioral reference; external web research is not required for this goal.
- Tokio is used only for asynchronous I/O and runtime primitives, `http` for protocol types, and Rustls for TLS.
- No replacement application framework such as Axum, Actix, Warp, Rocket, or Hyper routing is permitted.
- HTTP and HTTPS must share dispatcher, routing, controller, error, and response behavior.
- Completion claims require focused behavioral tests, including failure cases.

## Update log

- 2026-07-12: Created this ledger from the architecture handoff and re-verified the baseline: 29 framework tests pass and the test application builds. Recorded the persistent-buffer issue as the first concrete transport concern.
- 2026-07-12: Implemented persistent buffered connections, HTTP/1.0 and HTTP/1.1 keep-alive rules, connection timeouts, protocol errors, the TCP accept loop, shutdown signaling, and direct framework response serialization. Connected `Total::run_on()` to a native dispatcher for REST/API routes, params, AUTH, middleware, actions, contexts, and static fallback. Verification now passes with 33 tests plus the test-app build. Recorded native WebSockets and graceful draining as current blockers.
- 2026-07-12: Removed Axum, Tower, tower-http, and futures-util from source and dependency graphs; migrated all router tests to the native dispatcher; and added native HEAD/OPTIONS behavior. Verification passes with 34 tests and a clean test-app rebuild. Reviewed `../framework5/http.js`, `routing.js`, `controller.js`, and `websocket.js` for the controller pipeline, method handling, upgrade handshake, and framing model. Native WebSocket transport remains the next major blocker.
- 2026-07-12: Added native chunked request decoding with chunk extensions, trailers, decoded-body limits, framing validation, Content-Length conflict rejection, and preservation of pipelined bytes. Verification passes with 36 tests.
- 2026-07-12: Implemented the native RFC 6455 handshake and WebSocket frame pump directly on the buffered HTTP connection. Added text/binary input, outbound text, fragmentation, ping/pong, close, masking and protocol validation, plus a raw-byte masked-frame echo fixture. Verification passes with 38 tests and the test application builds.
- 2026-07-12: Added graceful shutdown signaling for every native connection. Idle keep-alive connections now close promptly, in-flight handlers finish and return `Connection: close`, and a configurable 30-second drain deadline bounds shutdown before remaining tasks are aborted. Two focused lifecycle tests bring verification to 40 passing tests; formatting and the test-application build also pass.
- 2026-07-12: Added native HTTPS with Rustls 0.22, PEM certificate/private-key loading, HTTP/1.1 ALPN, timeout-bounded handshakes, TLS close notifications, and the same native dispatcher and graceful-drain lifecycle used by HTTP. `Total::run_on()` enables TLS through `https_cert`/`https_key` or `tls_cert`/`tls_key`. An encrypted in-memory handshake/request fixture and missing-file failure test bring verification to 42 passing tests; formatting and the test-application build pass.
- 2026-07-12: Added configurable request-handler deadlines with the Total.js five-second default. Expired handler futures are dropped for cooperative cancellation and return 503 Service Unavailable. A focused cancellation/drop test brings verification to 43 passing tests; formatting and the offline test-application build pass.
- 2026-07-12: Completed the Total.js configuration convention: built-in defaults; `.env`, `config`, debug/release, plugin, and version loading; all safe typed annotations; base64/hex decoding; persistent Generate/Hash values; `$root`, `$api`, `$httpfiles`, `$timezone`, `$cryptoiv`, totalapi, and SMTP transformations; configured directories; typed accessors; global `CONF`; and reload/reconfigure hooks. Four parity fixtures bring verification to 47 passing tests. Formatting, testapp, and proofapp builds pass offline. `(Eval)` deliberately parses JSON rather than executing arbitrary JavaScript.
