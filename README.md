# total5

Total.rs is a Rust HTTP/API framework core inspired by Total.js v5. The first
milestone focuses on fast backend APIs with Total-style structure:

> **Architecture status:** ordinary live HTTP now uses the framework-owned
> Tokio TCP transport and Total.rs dispatcher. Axum, Tower, and tower-http have
> been removed. Native WebSocket upgrades/framing and chunked bodies now work;
> Advanced HTTP behavior and WebSocket parity remain. The authoritative
> behavioral reference is the local `../framework5` source tree, especially
> `http.js`, `routing.js`, `controller.js`, and `websocket.js`.

- convention discovery for `src/definitions`, `src/modules`, `src/services`,
  `src/schemas`, `src/controllers`, and `src/plugins/*`
- one-line application startup with `total5::main!()`
- one-line development startup with `total5::dev!()`
- automatic `config` loading and common directory preparation
- convention folders through `TPath`
- Total-style declarations with `INSTALL!`, `ROUTE!`, `SCHEMA!`, `ACTION!`,
  `NEWACTION!`, `MIDDLEWARE!`, `AUTH!`, `ON!`, `CONF!`, `PLUGIN!`,
  `WEBSOCKET!`, and `FLOWSTREAM!`
- Total.js-style schema validation strings with required fields, enums, typed
  arrays, inline objects, common built-in validators, and input transformation
- in-process Flow/FlowStream primitives with discovered stream declarations,
  inputs, and RPC handlers
- Total.js-like utility traits for strings, numbers, dates, and arrays
- static files, WebSocket routes, AUTH hooks, DATA/NoSQL, FileStorage, and
  Postgres connection support with `--features postgres`
- lower-level installable controllers/plugins remain available through
  `app.install(...)`
- route groups through `app.group("/api", |group| { ... })`
- middleware that can enrich or short-circuit request contexts
- request context with params, query, headers, JSON body helpers, and config
- JSON/text/empty responses
- named actions callable from handlers
- safe static files from `public/`

```rust
use serde_json::json;
use total5::{route, Context, Error, Response, Total};

#[tokio::main]
async fn main() -> Result<(), total5::Error> {
    let mut app = Total::new();
    app.config_mut().port = 5000;

    app.install(controllers)?;

    app.run().await
}

fn controllers(app: &mut Total) -> Result<(), Error> {
    app.group("/products", |group| {
        route!(group, "GET /{id}/", product_read)?;
        Ok(())
    })?;
    Ok(())
}

async fn product_read(ctx: Context) -> Response {
    Response::success(json!({ "id": ctx.param("id") }))
}
```

## Convention app

Applications can let Total.rs discover their structure at compile time:

```toml
# Cargo.toml
[dependencies]
total5 = { path = "../total5" }

[build-dependencies]
total5 = { path = "../total5", features = ["build"] }
```

```rust
// build.rs
fn main() -> std::io::Result<()> {
    total5::build::discover()
}
```

```rust
// src/main.rs
total5::main!();
```

For development mode:

```rust
// src/main.rs
total5::dev!();
```

Then create normal application files. Every discovered file that contains either
`total5::INSTALL!` or `pub fn install(app: &mut total5::Total) -> Result<(), total5::Error>`
is loaded in framework order: definitions, modules, services, flowstreams,
schemas, controllers, then plugins.

```rust
// src/schemas/products.rs
use serde_json::json;

total5::INSTALL! {
    SCHEMA!("Products", {
        ACTION!("read", params: "*id:String", |_ctx, input| async move {
            Ok(json!({ "id": input["id"], "name": "Product detail" }))
        });
    });
}
```

```rust
// src/controllers/products.rs
total5::INSTALL! {
    ROUTE!("GET /products/{id}/ --> Products/read");
    ROUTE!("API /api/ -products_read/{id} --> Products/read");
}
```

Total.js API routes share a single POST endpoint. The `schema` field selects
the operation; path parameters and query values are merged with `data` before
the action runs:

```bash
curl -X POST http://127.0.0.1:5000/api/ \
  -H 'content-type: application/json' \
  -d '{"schema":"products_read/123?detail=full","data":{}}'
```

Use `?` as the route base to use the configured `$api` endpoint (default
`/api/`): `ROUTE!("API ? -products_read/{id} --> Products/read");`.

`Context` is the Rust equivalent of the Total.js `$` controller passed to
routes, AUTH handlers, middleware, schemas, and actions. It exposes request
params/query/body/headers plus `user()`, `model()`/`value()`, `url()`, `ip()`,
and `ua()`. AUTH handlers populate the controller with `set_user()`; `+GET` or
`+API` routes require that user, while `-GET` or `-API` routes require a guest.

```rust
// src/definitions/app.rs
total5::INSTALL! {
    CONF!(name = "Products API");
    CONF!(port = 5000);

    MIDDLEWARE!(|mut ctx| async move {
        ctx.set("request_source", "products")?;
        Ok(ctx)
    });

    ON!("ready", |_app| {
        println!("ready");
        Ok(())
    });
}
```

This keeps the Total.js principle intact without a dynamic loader: the framework
understands the project from its structure, while Rust validates the discovered
modules during compilation.

## Roadmap to Total.js v5 parity

The goal is a native Rust framework with Total.js-compatible behavior, not a
wrapper around another web framework. Work is accepted only with conformance
tests derived from the local `../framework5` implementation.

### Current feature status

| Feature | Status | Current implementation |
|---|---|---|
| Web server | In migration | The application runs through a framework-owned HTTP/1.0 and HTTP/1.1 parser, Content-Length/chunked body decoder, buffered keep-alive connection loop, graceful draining, response writer, TCP/Rustls accept loops, and one Total.rs dispatcher for HTTP and HTTPS. Uploads, ranges, and other advanced behaviors remain. |
| Configuration | Compatible | Total.js defaults, `.env`, base/mode/plugin overlays, version files, typed values, generated-value persistence, special transformations, directory remapping, global snapshots, and runtime reconfiguration work. `(Eval)` accepts JSON but intentionally does not execute JavaScript. |
| Proxy | Missing | No equivalent to `../framework5/proxy.js`. |
| Static files | Partial | Safe public-file serving and cache headers; advanced file routes, ranges, transforms, localization, compression, and streaming are missing. |
| Request routing | Partial | REST and API routes, parameters, middleware, AUTH flags, schemas, actions, and slash normalization work. FILE/SOCKET routes, priorities, full flags, action chains, timeouts, virtual routes, and several fallback behaviors are missing. |
| SMTP sender | Missing | No equivalent to `../framework5/mail.js`. |
| TextDB/NoSQL | Minimal | Basic JSON-file CRUD exists. It is not yet a TextDB-compatible query, streaming, indexing, view, backup, locking, or recovery engine. |
| Image handling | Missing | No GraphicsMagick/ImageMagick pipeline equivalent to `../framework5/images.js`. |
| Data models | Partial | Schemas, actions, validation strings, params, query validation, and controller models exist. Outputs, permissions, transformations, publication, workflows, and complete action chaining are missing. |
| WebSocket server | Partial | Native RFC 6455 upgrade, masked frame decoding, text/binary delivery, outbound text, fragmentation, ping/pong, close, and protocol limits work. Compression, subprotocols, route context/AUTH parity, connection collections, and outbound clients remain. |
| WebSocket client | Missing | No outbound WebSocket client. |
| Cron | Missing | No parser or scheduler equivalent to `../framework5/cron.js`. |
| Workers | Missing | No worker process/thread lifecycle or IPC equivalent to `../framework5/workers.js`. |
| File Storage | Minimal | Save, read, stat, and remove work. Metadata queries, streaming, backups, image operations, ranges, and cleanup policies are missing. |
| Data Flow | Minimal | In-memory FlowStream inputs and RPC work. Component execution, persistence, schemas, messaging, workers, and designer metadata are missing. |
| Components | Missing | Plugin metadata is not the component runtime from `../framework5/components.js`. |
| Clustering | Missing | No equivalent to `../framework5/cluster.js`. |
| Source-code bundling | Missing | Compile-time convention discovery exists, but it is not the source bundler from `../framework5/bundles.js`. |
| Number/String/Date/Array prototypes | Partial | A small type-safe subset exists; most helpers from `../framework5/utils.js` are not implemented. |

This table must be updated whenever a subsystem changes status. A feature can
be marked complete only after its completion gates and local-reference parity
tests pass.

### Architecture rules

- Total.rs owns the HTTP lifecycle: request parsing, limits, controller
  creation, routing, authorization, middleware, static files, errors, response
  writing, connection reuse, upgrades, and statistics.
- Tokio provides asynchronous TCP I/O. The `http` crate provides HTTP types,
  and Rustls provides TLS primitives; none of them define framework behavior.
- Axum, Actix, Rocket, Warp, and similar application frameworks are forbidden
  in the final server implementation.
- HTTP and HTTPS must use the same Total.rs dispatcher after transport setup.
- `../framework5` is the main reference. External documentation explains the
  public contract but does not override source behavior.
- A subsystem is never marked complete because similarly named structs or
  methods exist. Its behavior and failure cases must pass parity tests.

### Phase 1 — framework-owned web server

- Replace Axum routing and serving with a Total.rs server built on Tokio TCP,
  `http` types, and Rustls.
- Implement incremental HTTP/1.1 parsing with strict header/body limits,
  keep-alive, timeouts, malformed-request handling, and graceful disconnects.
- Add HTTPS listeners, certificate/key loading, TLS handshakes, and the same
  dispatcher used by plain HTTP.
- Move route lookup, parameter extraction, API routing, AUTH, middleware,
  controller creation, static-file fallback, and response serialization into
  framework-owned code.
- Implement HEAD/OPTIONS, content length, chunked bodies, compression, ranges,
  uploads, cookies, CORS, request cancellation, fallback routes, and request/
  response statistics.
- Implement WebSocket upgrade and framing without coupling routing to a web
  application framework; add an outbound WebSocket client.
- Remove the Axum and tower-http dependencies only after HTTP, HTTPS, static
  files, API routes, AUTH, and WebSockets pass end-to-end tests.

### Phase 2 — routing and controller parity

- Complete Total.js route parsing: HTTP, API, FILE, SOCKET, wildcard, typed
  params, flags, size/time limits, middleware lists, priorities, fallbacks,
  virtual routes, proxy lookup, and action chains.
- Expand `Context`, the Rust equivalent of `$`, across routes, AUTH,
  middleware, schemas, actions, operations, and WebSockets.
- Add controller response methods, cookies, redirects, files, streams, proxy,
  audit, cancellation, transformations, localization, and action callers.
- Complete action input/params/query/output validation, permissions, partial
  validation, publication, chaining, and consistent error handling.

### Phase 3 — persistence and files

- Replace the basic JSON-file `Data` helper with a TextDB-compatible engine:
  query builders, readers, streaming, filters, sorting, pagination, counters,
  scalar operations, indexes, views, backups, locking, and recovery.
- Complete FileStorage metadata, querying, streaming, deduplication, backups,
  image integration, range reads, and cleanup behavior.
- Complete SQL QueryBuilder execution and supported database adapters.

### Phase 4 — platform services

- SMTP client and message builder compatible with `mail.js`.
- HTTP/HTTPS proxying, keep-alive pools, forwarding, streaming, and timeouts.
- GraphicsMagick/ImageMagick command pipelines and image response helpers.
- Cron parser/scheduler, workers, clustering, IPC, graceful restart, and usage
  statistics.

### Phase 5 — application architecture

- Full Flow and FlowStream runtime, component lifecycle, schemas, messages,
  RPC, persistence, workers, and designer metadata.
- Components, plugins, modules, services, TMS, transformations, templates,
  views, localization, source maps, minification, and source-code bundles.
- Expand Number, String, Date, Array, and utility compatibility where Rust
  equivalents are useful and type-safe.

### Completion gates

Each phase requires:

1. unit tests for parsers, state transitions, limits, and failures;
2. protocol tests using raw TCP/TLS clients rather than framework adapters;
3. test-app coverage for the public API;
4. behavior comparisons with focused fixtures from `../framework5`;
5. no placeholder implementation presented as feature parity.

## Configuration

Total.rs follows the Total.js configuration load chain. `Total::convention()`
loads `.env`, `config`, the applicable `config-debug` or `config-release`
overlay, sorted `plugins/*/config` overlays, and the first line of `version`.
`Total::convention_dev()` selects the debug overlay. Existing process
environment variables take precedence over `.env` values.

Put a file named `config` in the application root:

```txt
name : Products API
port : 5000
debug : true
database : postgres://user:pass@example.com/app
nested (Object) : {"enabled":true}
token (Env) : API_TOKEN
roles (Array) : admin, editor
generated_secret (Generate) : 32
```

Blank lines and comments with `#` or `//` are ignored. Parsed values are stored
in the application config, available through `ctx.conf()`/`ctx.config()`, and
mirrored to the global `total5::CONF` snapshot. Supported annotations include
String, Boolean, Number/Float/Double/Currency, Int, Array, Object/JSON, Env,
Config, Date/Time/DateTime, Random, Generate, Hash, and JSON-safe Eval. Values
prefixed with `base64 ` or `hex ` are decoded. Generate/Hash values persist in
`databases/config.json`, matching Total.js restart behavior.

The built-in Total.js `$` configuration defaults are present, including HTTP,
WebSocket, cookie, proxy, TAPI/TMS, minification, caching, and directory keys.
Special `$root`, `$api`, `$httpfiles`, `$timezone`, `$cryptoiv`, `totalapi`, and
SMTP transformations are applied. `$dir*` and `directory_*` values remap the
framework path helpers. Applications can call `reload_config()` and subscribe
with `on_reconfigure()` for runtime reload handling.

Unlike JavaScript Total.js, `(Eval)` intentionally accepts JSON values instead
of executing arbitrary JavaScript from configuration files.

## Proof Application

`proofapp/` is the living API specification for the intended developer
experience. It demonstrates:

- `config` discovery
- one-line development startup
- discovered definitions, services, schemas, controllers, and plugins
- discovered flowstreams with RPC/input handlers
- AUTH, DATA/NoSQL, FileStorage, and WebSocket declarations
- Total-style declarations instead of manual router wiring
- plugin metadata and plugin-owned routes

Run it with:

```bash
cd proofapp
cargo run
```
