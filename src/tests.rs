use super::*;
use http::Request;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

struct Body(Bytes);

impl Body {
    fn empty() -> Bytes {
        Bytes::new()
    }
}

impl From<&str> for Body {
    fn from(value: &str) -> Self {
        Self(Bytes::copy_from_slice(value.as_bytes()))
    }
}

impl From<Body> for Bytes {
    fn from(value: Body) -> Self {
        value.0
    }
}

mod body {
    use bytes::Bytes;

    pub async fn to_bytes(body: Bytes, _limit: usize) -> Result<Bytes, std::convert::Infallible> {
        Ok(body)
    }
}

#[test]
fn parses_route_expression() {
    let (method, path, auth) = parse_route_expression("GET /products/{id}/").unwrap();
    assert_eq!(method, RouteMethod::Get);
    assert_eq!(path, "/products/{id}/");
    assert_eq!(auth, RouteAuth::Any);
}

#[test]
fn matches_total_route_params_natively() {
    let params = match_native_path(
        "/products/{id}/comments/{comment}/",
        "/products/10/comments/20/",
    )
    .unwrap();
    assert_eq!(params["id"], "10");
    assert_eq!(params["comment"], "20");
}

#[test]
fn rejects_static_traversal() {
    assert!(safe_join(Path::new("/tmp/public"), "../secret").is_err());
    assert!(safe_join(Path::new("/tmp/public"), "css/app.css").is_ok());
}

#[tokio::test]
async fn serves_route_params_end_to_end() {
    let mut app = Total::new();
    app.route("GET /products/{id}/", |ctx| async move {
        Response::success(json!({ "id": ctx.param("id") }))
    })
    .unwrap();

    let router = app.router();
    let response = router
        .clone()
        .oneshot(
            Request::builder()
                .uri("/products/123/")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let bytes = body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let payload: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(payload["value"]["id"], "123");
}

#[tokio::test]
async fn native_server_dispatches_route_params_over_raw_http() {
    let mut app = Total::new();
    app.middleware(|mut ctx| async move {
        ctx.set("transport", "native").unwrap();
        Ok(ctx)
    });
    app.route("GET /products/{id}/", |ctx| async move {
        Response::success(json!({
            "id": ctx.param("id"),
            "transport": ctx.get("transport")
        }))
    })
    .unwrap();
    let dispatcher = app.native_dispatcher();
    let (mut client, server_stream) = tokio::io::duplex(4096);
    let server = tokio::spawn(server::serve_connection(
        server_stream,
        server::ServerConfig::default(),
        move |request| {
            let dispatcher = dispatcher.clone();
            async move { dispatcher.dispatch(request).await }
        },
    ));

    client
        .write_all(
            b"GET /products/native-123/ HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n",
        )
        .await
        .unwrap();
    let mut response = Vec::new();
    client.read_to_end(&mut response).await.unwrap();
    server.await.unwrap().unwrap();

    let response = String::from_utf8(response).unwrap();
    assert!(response.starts_with("HTTP/1.1 200 OK\r\n"));
    assert!(response.contains("\"id\":\"native-123\""));
    assert!(response.contains("\"transport\":\"native\""));
}

#[tokio::test]
async fn native_dispatcher_supports_head_and_options() {
    let mut app = Total::new();
    app.route("GET /status/", |_| async { Response::text("healthy") })
        .unwrap();
    let dispatcher = app.native_dispatcher();

    let head = dispatcher
        .dispatch(
            http::Request::builder()
                .method(Method::HEAD)
                .uri("/status/")
                .body(Bytes::new())
                .unwrap(),
        )
        .await;
    assert_eq!(head.status(), StatusCode::OK);
    assert_eq!(head.headers()[header::CONTENT_LENGTH], "7");
    assert!(head.body().is_empty());

    let options = dispatcher
        .dispatch(
            http::Request::builder()
                .method(Method::OPTIONS)
                .uri("/status/")
                .body(Bytes::new())
                .unwrap(),
        )
        .await;
    assert_eq!(options.status(), StatusCode::NO_CONTENT);
    assert_eq!(options.headers()[header::ALLOW], "GET, HEAD, OPTIONS");
    assert!(options.body().is_empty());
}

#[test]
fn computes_rfc6455_websocket_accept_key() {
    assert_eq!(
        websocket_accept("dGhlIHNhbXBsZSBub25jZQ=="),
        "s3pPLMBiTxaQ9kYGzzhZRbK+xOo="
    );
}

#[tokio::test]
async fn native_websocket_upgrades_and_echoes_masked_frame() {
    let mut app = Total::new();
    app.websocket("/echo/", |mut socket| async move {
        if let Some(Ok(message)) = socket.recv().await {
            socket.send_text(message).await.unwrap();
        }
    })
    .unwrap();
    let dispatcher = app.native_dispatcher();
    let (mut client, server_stream) = tokio::io::duplex(8192);
    let server = tokio::spawn(server::serve_connection(
        server_stream,
        server::ServerConfig::default(),
        move |request| {
            let dispatcher = dispatcher.clone();
            async move { dispatcher.dispatch(request).await }
        },
    ));

    let mut request = b"GET /echo/ HTTP/1.1\r\nHost: localhost\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Version: 13\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\n\r\n".to_vec();
    let mask = [1_u8, 2, 3, 4];
    request.extend_from_slice(&[0x81, 0x85]);
    request.extend_from_slice(&mask);
    request.extend(
        b"hello"
            .iter()
            .enumerate()
            .map(|(index, byte)| byte ^ mask[index % 4]),
    );
    client.write_all(&request).await.unwrap();

    let mut output = Vec::new();
    client.read_to_end(&mut output).await.unwrap();
    server.await.unwrap().unwrap();
    let header_end = output
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .unwrap()
        + 4;
    let headers = String::from_utf8_lossy(&output[..header_end]);
    assert!(headers.starts_with("HTTP/1.1 101 Switching Protocols\r\n"));
    assert!(headers.contains("sec-websocket-accept: s3pPLMBiTxaQ9kYGzzhZRbK+xOo=\r\n"));
    assert!(output[header_end..]
        .windows(7)
        .any(|frame| frame == b"\x81\x05hello"));
}

#[tokio::test]
async fn websocket_negotiates_protocol_and_carries_route_auth_context() {
    let mut app = Total::new();
    app.auth(|mut ctx| async move {
        if ctx.headers.contains_key(header::AUTHORIZATION) {
            ctx.set_user(json!({ "id": "member" })).unwrap();
        }
        Ok(ctx)
    });
    app.websocket_options(
        "/room/{id}/",
        &["total-json", "chat"],
        true,
        |mut socket| async move {
            socket
                .send_text(format!(
                    "{}:{}:{}:{}",
                    socket.param("id").unwrap(),
                    socket.query("token").unwrap(),
                    socket.protocol().unwrap(),
                    socket.user().unwrap()["id"].as_str().unwrap()
                ))
                .await
                .unwrap();
        },
    )
    .unwrap();
    let dispatcher = app.native_dispatcher();
    let (mut client, server_stream) = tokio::io::duplex(8192);
    let server = tokio::spawn(server::serve_connection(
        server_stream,
        server::ServerConfig::default(),
        {
            let dispatcher = dispatcher.clone();
            move |request| {
                let dispatcher = dispatcher.clone();
                async move { dispatcher.dispatch(request).await }
            }
        },
    ));
    client
            .write_all(b"GET /room/42/?token=abc HTTP/1.1\r\nHost: localhost\r\nAuthorization: Bearer x\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Version: 13\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\nSec-WebSocket-Protocol: unknown, chat\r\n\r\n")
            .await
            .unwrap();
    let mut output = Vec::new();
    client.read_to_end(&mut output).await.unwrap();
    server.await.unwrap().unwrap();
    let header_end = output
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .unwrap()
        + 4;
    let headers = String::from_utf8_lossy(&output[..header_end]);
    assert!(headers.contains("sec-websocket-protocol: chat\r\n"));
    assert!(output[header_end..]
        .windows(b"42:abc:chat:member".len())
        .any(|window| window == b"42:abc:chat:member"));
    assert_eq!(dispatcher.statistics().websocket_connections, 0);
}

#[tokio::test]
async fn serves_static_files_and_blocks_traversal() {
    let root = unique_temp_dir();
    let public = root.join("public");
    std::fs::create_dir_all(&public).unwrap();
    std::fs::write(public.join("app.txt"), "hello").unwrap();

    let app = Total::new().root(&root).router();
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/app.txt")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let response = app
        .oneshot(
            Request::builder()
                .uri("/../secret.txt")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);

    std::fs::remove_dir_all(root).ok();
}

#[tokio::test]
async fn compresses_eligible_responses_and_respects_the_config_switch() {
    let mut app = Total::new();
    app.route("GET /large/", |_ctx| async {
        Response::text("x".repeat(600))
    })
    .unwrap();
    let response = app
        .router()
        .oneshot(
            Request::builder()
                .uri("/large/")
                .header(header::ACCEPT_ENCODING, "br, gzip")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.headers()[header::CONTENT_ENCODING], "gzip");
    assert_eq!(response.headers()[header::VARY], "Accept-Encoding");
    let mut decoder = flate2::read::GzDecoder::new(response.body().as_ref());
    let mut decoded = Vec::new();
    std::io::Read::read_to_end(&mut decoder, &mut decoded).unwrap();
    assert_eq!(decoded, vec![b'x'; 600]);

    let mut app = Total::new();
    app.config_mut().set("$httpcompress", false).unwrap();
    app.route("GET /large/", |_ctx| async {
        Response::text("x".repeat(600))
    })
    .unwrap();
    let response = app
        .router()
        .oneshot(
            Request::builder()
                .uri("/large/")
                .header(header::ACCEPT_ENCODING, "gzip")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert!(!response.headers().contains_key(header::CONTENT_ENCODING));
    assert_eq!(response.body().len(), 600);
}

#[tokio::test]
async fn serves_static_byte_ranges_and_rejects_unsatisfiable_ranges() {
    let root = unique_temp_dir();
    std::fs::create_dir_all(root.join("public")).unwrap();
    std::fs::write(root.join("public/data.txt"), "0123456789").unwrap();
    let app = Total::new().root(&root).router();

    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/data.txt")
                .header(header::RANGE, "bytes=2-5")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::PARTIAL_CONTENT);
    assert_eq!(response.headers()[header::CONTENT_RANGE], "bytes 2-5/10");
    assert_eq!(response.headers()[header::CONTENT_LENGTH], "4");
    assert_eq!(response.body(), &Bytes::from_static(b"2345"));

    let response = app
        .oneshot(
            Request::builder()
                .uri("/data.txt")
                .header(header::RANGE, "bytes=99-")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::RANGE_NOT_SATISFIABLE);
    assert_eq!(response.headers()[header::CONTENT_RANGE], "bytes */10");
    assert!(response.body().is_empty());
    std::fs::remove_dir_all(root).ok();
}

#[tokio::test]
async fn parses_bounded_multipart_fields_and_files() {
    let mut app = Total::new();
    app.route("POST /upload/", |ctx| async move {
        let file = &ctx.files()[0];
        Response::json(json!({
            "title": ctx.field("title"),
            "field": file.field,
            "filename": file.filename,
            "content_type": file.content_type,
            "data": String::from_utf8_lossy(&file.data),
        }))
    })
    .unwrap();
    let boundary = "totalrs-boundary";
    let body = format!(
            "--{boundary}\r\nContent-Disposition: form-data; name=\"title\"\r\n\r\nExample\r\n--{boundary}\r\nContent-Disposition: form-data; name=\"asset\"; filename=\"../hello.txt\"\r\nContent-Type: text/plain\r\n\r\nhello\r\n--{boundary}--\r\n"
        );
    let response = app
        .router()
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/upload/")
                .header(
                    header::CONTENT_TYPE,
                    format!("multipart/form-data; boundary={boundary}"),
                )
                .body(Bytes::from(body))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let value: Value = serde_json::from_slice(response.body()).unwrap();
    assert_eq!(value["title"], "Example");
    assert_eq!(value["field"], "asset");
    assert_eq!(value["filename"], "hello.txt");
    assert_eq!(value["content_type"], "text/plain");
    assert_eq!(value["data"], "hello");
}

#[test]
fn rejects_multipart_field_limit_violations() {
    let mut config = Config::default();
    config.set("$httpmaxkeys", 1).unwrap();
    let boundary = "limit";
    let body = format!(
            "--{boundary}\r\nContent-Disposition: form-data; name=\"one\"\r\n\r\n1\r\n--{boundary}\r\nContent-Disposition: form-data; name=\"two\"\r\n\r\n2\r\n--{boundary}--\r\n"
        );
    let error = parse_multipart(
        body.as_bytes(),
        &format!("multipart/form-data; boundary={boundary}"),
        &config,
    )
    .unwrap_err();
    assert_eq!(error.status, StatusCode::BAD_REQUEST);
    assert!(error.message.contains("too many"));
}

#[tokio::test]
async fn serializes_and_verifies_signed_cookies() {
    let mut app = Total::new();
    app.config_mut().set("secret", "cookie-secret").unwrap();
    app.route("GET /cookie/", |ctx| async move {
        Response::json(json!({ "session": ctx.signed_cookie("session") }))
            .cookie(
                "theme",
                "dark mode",
                CookieOptions {
                    secure: true,
                    max_age: Some(3600),
                    ..CookieOptions::default()
                },
            )
            .unwrap()
    })
    .unwrap();
    let signed = format!("user-1.{}", cookie_signature("user-1", "cookie-secret"));
    let response = app
        .router()
        .oneshot(
            Request::builder()
                .uri("/cookie/")
                .header(header::COOKIE, format!("session={signed}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let cookie = response.headers()[header::SET_COOKIE].to_str().unwrap();
    assert!(cookie.starts_with("theme=dark%20mode; Path=/"));
    assert!(cookie.contains("Max-Age=3600"));
    assert!(cookie.contains("HttpOnly"));
    assert!(cookie.contains("Secure"));
    assert!(cookie.contains("SameSite=Lax"));
    let value: Value = serde_json::from_slice(response.body()).unwrap();
    assert_eq!(value["session"], "user-1");
}

#[tokio::test]
async fn applies_cors_to_regular_and_preflight_responses() {
    let mut app = Total::new();
    app.config_mut()
        .set("$cors", "https://app.example.com,*://trusted.test")
        .unwrap();
    app.route("GET /cors/", |_ctx| async { Response::text("ok") })
        .unwrap();
    let router = app.router();
    let response = router
        .clone()
        .oneshot(
            Request::builder()
                .uri("/cors/")
                .header(header::ORIGIN, "https://app.example.com")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(
        response.headers()[header::ACCESS_CONTROL_ALLOW_ORIGIN],
        "https://app.example.com"
    );

    let response = router
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::OPTIONS)
                .uri("/cors/")
                .header(header::ORIGIN, "https://app.example.com")
                .header(header::ACCESS_CONTROL_REQUEST_METHOD, "GET")
                .header(header::ACCESS_CONTROL_REQUEST_HEADERS, "authorization")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    assert_eq!(
        response.headers()[header::ACCESS_CONTROL_ALLOW_METHODS],
        "GET"
    );
    assert_eq!(
        response.headers()[header::ACCESS_CONTROL_ALLOW_HEADERS],
        "authorization"
    );

    let response = router
        .oneshot(
            Request::builder()
                .method(Method::OPTIONS)
                .uri("/cors/")
                .header(header::ORIGIN, "https://denied.example")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn enforces_blacklists_request_limits_and_tracks_statistics() {
    let mut blocked_app = Total::new();
    blocked_app
        .config_mut()
        .set("$blacklist", "10.0.*,192.168.1.5")
        .unwrap();
    blocked_app
        .route("POST /stats/", |_ctx| async { Response::text("accepted") })
        .unwrap();
    let blocked = blocked_app.router();
    let response = blocked
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/stats/")
                .header("x-forwarded-for", "10.0.4.2")
                .body(Bytes::from_static(b"payload"))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    assert_eq!(blocked.statistics().blocked, 1);

    let mut limited_app = Total::new();
    limited_app.config_mut().set("$httpreqlimit", 1).unwrap();
    limited_app
        .route("POST /stats/", |_ctx| async { Response::text("accepted") })
        .unwrap();
    let limited = limited_app.router();
    for expected in [StatusCode::OK, StatusCode::TOO_MANY_REQUESTS] {
        let response = limited
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/stats/")
                    .header("x-real-ip", "203.0.113.10")
                    .body(Bytes::from_static(b"1234"))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), expected);
    }
    let stats = limited.statistics();
    assert_eq!(stats.requests, 2);
    assert_eq!(stats.pending, 0);
    assert_eq!(stats.throttled, 1);
    assert_eq!(stats.responses_2xx, 1);
    assert_eq!(stats.responses_4xx, 1);
    assert_eq!(stats.downloaded_bytes, 8);
    assert_eq!(stats.uploaded_bytes, 8);
}

#[tokio::test]
async fn installs_grouped_routes() {
    fn controller(app: &mut Total) -> Result<(), Error> {
        app.group("/api", |group| {
            group.route("GET /status/", |_ctx| async {
                Response::success(json!({ "ok": true }))
            })?;
            Ok(())
        })?;
        Ok(())
    }

    let mut app = Total::new();
    app.install(controller).unwrap();

    let router = app.router();
    let response = router
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/status/")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn middleware_can_enrich_context() {
    let mut app = Total::new();
    app.middleware(|mut ctx| async move {
        ctx.set("tenant", "acme").unwrap();
        Ok(ctx)
    });
    app.route("GET /tenant/", |ctx| async move {
        Response::success(json!({ "tenant": ctx.get("tenant") }))
    })
    .unwrap();

    let response = app
        .router()
        .oneshot(
            Request::builder()
                .uri("/tenant/")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let bytes = body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let payload: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(payload["value"]["tenant"], "acme");
}

#[tokio::test]
async fn middleware_can_short_circuit() {
    let mut app = Total::new();
    app.middleware(|_ctx| async move {
        Err(
            Response::json(json!({ "success": false, "error": "blocked" }))
                .status(StatusCode::UNAUTHORIZED),
        )
    });
    app.route("GET /private/", |_ctx| async {
        Response::success(json!({ "secret": true }))
    })
    .unwrap();

    let response = app
        .router()
        .oneshot(
            Request::builder()
                .uri("/private/")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn schema_actions_validate_input() {
    let mut app = Total::new();
    app.schema("Users", |schema| {
        schema.action_with(
            "create",
            Validator::new()
                .required("email", FieldKind::Email)
                .rule(FieldRule::new("name", FieldKind::String).required().min(2)),
            |_ctx, input| async move { Ok(json!({ "email": input["email"] })) },
        );
        Ok(())
    })
    .unwrap();
    app.route("POST /users/", |ctx| async move {
        let input: Value = ctx.body().unwrap_or_else(|_| json!({}));
        ctx.action_success("Users/create", input).await
    })
    .unwrap();

    let response = app
        .router()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/users/")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"email":"bad","name":"A"}"#))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let bytes = body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let payload: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(payload["success"], false);
    assert_eq!(payload["errors"].as_array().unwrap().len(), 2);
}

#[tokio::test]
async fn schema_actions_return_success() {
    let mut app = Total::new();
    app.schema("Products", |schema| {
        schema.action_with(
            "read",
            Validator::new().required("id", FieldKind::String),
            |_ctx, input| async move { Ok(json!({ "id": input["id"] })) },
        );
        Ok(())
    })
    .unwrap();
    app.route("GET /products/{id}/", |ctx| async move {
        ctx.action_success("Products/read", json!({ "id": ctx.param("id") }))
            .await
    })
    .unwrap();

    let response = app
        .router()
        .oneshot(
            Request::builder()
                .uri("/products/123/")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let bytes = body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let payload: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(payload["value"]["id"], "123");
}

#[tokio::test]
async fn compat_route_calls_action_target() {
    let mut app = Total::new();
    app.schema("Products", |schema| {
        schema.action_options(
            "read",
            ActionOptions::new().params("*id:String").unwrap(),
            |_ctx, input| async move { Ok(json!({ "id": input["id"] })) },
        );
        Ok(())
    })
    .unwrap();
    route!(app, "GET /products/{id}/ --> Products/read").unwrap();

    let response = app
        .router()
        .oneshot(
            Request::builder()
                .uri("/products/123/")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let bytes = body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let payload: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(payload["value"]["id"], "123");
}

#[test]
fn parses_api_compat_routes() {
    let route = CompatRoute::parse("+API /admin/ #admin_jobs_update/{id} --> Jobs/update").unwrap();
    assert_eq!(route.method, RouteMethod::Post);
    assert_eq!(route.path, "/admin/");
    assert_eq!(route.action, "Jobs/update");
    assert_eq!(
        route.api,
        Some(CompatApiEndpoint {
            name: "admin_jobs_update".to_string(),
            params: vec![(1, "id".to_string())]
        })
    );
}

#[tokio::test]
async fn total_api_routes_dispatch_schema_data_params_and_query() {
    let mut app = Total::new();
    app.auth(|mut ctx| async move {
        if ctx.headers.contains_key("authorization") {
            ctx.set_user(json!({ "id": "tester" })).unwrap();
        }
        Ok(ctx)
    });
    app.action("Products/read", |_ctx, input| async move { Ok(input) });
    app.route_compat("API /api/ -products_read/{id} --> Products/read")
        .unwrap();
    app.route_compat("+API /api/ +products_private/{id} --> Products/read")
        .unwrap();

    let router = app.router();
    let response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/")
                .header("content-type", "application/json")
                .body(Body::from(
                    r#"{"schema":"products_read/123?search=Total%2Ejs","data":{"active":true}}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let bytes = body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let payload: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(payload["value"]["id"], "123");
    assert_eq!(payload["value"]["search"], "Total.js");
    assert_eq!(payload["value"]["active"], true);

    let unauthorized = router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"schema":"products_private/123","data":{}}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(unauthorized.status(), StatusCode::UNAUTHORIZED);

    let authorized = router
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/")
                .header("content-type", "application/json")
                .header("authorization", "Bearer test")
                .body(Body::from(r#"{"schema":"products_private/123","data":{}}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(authorized.status(), StatusCode::OK);
}

#[test]
fn parses_total_schema_validation_strings() {
    let validator =
        Validator::parse("*id:UID,name:String(4),email:Email,status:{draft|published}").unwrap();
    assert!(validator
        .validate(&json!({
            "id": "abc",
            "name": "Test",
            "email": "a@b.com",
            "status": "draft"
        }))
        .is_ok());
    assert!(validator
        .validate(&json!({
            "name": "Too long",
            "email": "bad",
            "status": "other"
        }))
        .is_err());
}

#[test]
fn parses_total_schema_extended_validation_strings() {
    let validator = Validator::parse(
            "*number:Number,*email:Email,*phone:Phone,*boolean:Boolean,*uid:UID,*base64:Base64,*url:URL,*object:Object,*date:Date,*json:JSON,*datauri:DataURI,*zip:ZIP,*icon:Icon,*color:Color,*guid:GUID,*tinyint:TinyInt,*smallint:SmallInt,enums:{red|green|blue},inlineobject:{name:String,email:Email},arrayobj:[name:String,email:Email],tags:[String]",
        )
        .unwrap();

    let input = json!({
        "number": "12.5",
        "email": "a@b.com",
        "phone": "+1234567890",
        "boolean": "true",
        "uid": "abc",
        "base64": "YWJjZA==",
        "url": "https://example.com",
        "object": {},
        "date": "2026-07-09",
        "json": "{\"ok\":true}",
        "datauri": "data:text/plain;base64,YWJjZA==",
        "zip": "12345",
        "icon": "ti ti-check",
        "color": "#ffaa00",
        "guid": "550e8400-e29b-41d4-a716-446655440000",
        "tinyint": "10",
        "smallint": "300",
        "enums": "red",
        "inlineobject": { "name": "Peter", "email": "p@example.com" },
        "arrayobj": [{ "name": "Anna", "email": "a@example.com" }],
        "tags": ["a", "b"]
    });

    let output = validator.transform(input).unwrap();
    assert_eq!(output["number"], 12.5);
    assert_eq!(output["boolean"], true);
    assert_eq!(output["json"]["ok"], true);
    assert_eq!(output["tinyint"], 10);
}

#[test]
fn rejects_total_schema_nested_validation_errors() {
    let validator =
        Validator::parse("inline:{name:String,email:Email},items:[name:String,email:Email]")
            .unwrap();
    let err = validator
        .validate(&json!({
            "inline": { "name": "A", "email": "bad" },
            "items": [{ "name": "B", "email": "bad" }]
        }))
        .unwrap_err();
    assert!(err
        .validation
        .iter()
        .any(|item| item.field == "inline.email"));
    assert!(err
        .validation
        .iter()
        .any(|item| item.field == "items[0].email"));
}

#[test]
fn parses_total_config_files() {
    let root = unique_temp_dir();
    std::fs::create_dir_all(&root).unwrap();
    let path = root.join("config");
    std::fs::write(
            &path,
            "# app config\napp_name : Demo\nport : 5050 // listen port\ndebug : true\nnested (Object) : {\"ok\":true}\nurl : https://example.com/api\nquoted : \"value # kept\"\n",
        )
        .unwrap();

    let mut config = Config::default();
    config.load_total_config(&path).unwrap();

    assert_eq!(config.name, "Demo");
    assert_eq!(config.port, 5050);
    assert_eq!(config.get("debug"), Some(&Value::Bool(true)));
    assert_eq!(
        config.get("nested"),
        Some(&json!({
            "ok": true
        }))
    );
    assert_eq!(
        config.get("url"),
        Some(&Value::String("https://example.com/api".to_string()))
    );
    assert_eq!(
        config.get("quoted"),
        Some(&Value::String("value # kept".to_string()))
    );
    std::fs::remove_dir_all(root).ok();
}

#[test]
fn parses_total_config_types_and_persists_generated_values() {
    let root = unique_temp_dir();
    std::fs::create_dir_all(&root).unwrap();
    let env_name = format!("TOTAL5_CONFIG_TEST_{}", uuid::Uuid::new_v4().simple());
    std::env::set_var(&env_name, "from-env");
    let path = root.join("config");
    std::fs::write(
            &path,
            format!(
                "enabled (Boolean) : enabled\nitems (Array) : one, two,three\namount (Currency) : 12.50\nsource : copied\ncopy (Config) : source\nenvironment (Env) : {env_name}\ndate (Date) : 2026-07-12\ndecoded : base64 aGVsbG8=\nhexed : hex 776f726c64\ngenerated (Generate) : 18\nhashed (Hash) : 20\njson (JSON) : {{\"ok\":true}}\n"
            ),
        )
        .unwrap();

    let mut first = Config::default();
    first.load_total_config(&path).unwrap();
    assert_eq!(first.get("enabled"), Some(&json!(true)));
    assert_eq!(first.get("items"), Some(&json!(["one", "two", "three"])));
    assert_eq!(first.get("amount"), Some(&json!(12.5)));
    assert_eq!(first.get("copy"), Some(&json!("copied")));
    assert_eq!(first.get("environment"), Some(&json!("from-env")));
    assert_eq!(first.get("date"), Some(&json!("2026-07-12T00:00:00Z")));
    assert_eq!(first.get("decoded"), Some(&json!("hello")));
    assert_eq!(first.get("hexed"), Some(&json!("world")));
    assert_eq!(first.get("json"), Some(&json!({ "ok": true })));
    let generated = first.get("generated").cloned().unwrap();
    let hashed = first.get("hashed").cloned().unwrap();
    assert_eq!(generated.as_str().unwrap().len(), 18);
    assert_eq!(hashed.as_str().unwrap().len(), 20);
    assert!(root.join("databases/config.json").is_file());

    let mut second = Config::default();
    second.load_total_config(&path).unwrap();
    assert_eq!(second.get("generated"), Some(&generated));
    assert_eq!(second.get("hashed"), Some(&hashed));

    std::env::remove_var(env_name);
    std::fs::remove_dir_all(root).ok();
}

#[test]
fn convention_merges_env_mode_plugin_and_version_configuration() {
    let root = unique_temp_dir();
    std::fs::create_dir_all(root.join("plugins/catalog")).unwrap();
    let env_name = format!("TOTAL5_LOAD_TEST_{}", uuid::Uuid::new_v4().simple());
    std::fs::write(root.join(".env"), format!("{env_name}=loaded\n")).unwrap();
    std::fs::write(
        root.join("config"),
        format!("debug : true\nfrom_env (Env) : {env_name}\norder : base\n"),
    )
    .unwrap();
    std::fs::write(
        root.join("config-debug"),
        "order : debug\nmode_only : yes\n",
    )
    .unwrap();
    std::fs::write(root.join("config-release"), "order : release\n").unwrap();
    std::fs::write(
        root.join("plugins/catalog/config"),
        "order : plugin\nplugin_value : 42\n",
    )
    .unwrap();
    std::fs::write(root.join("version"), "2.5.1\nignored\n").unwrap();

    let mut app = Total::new().root(&root);
    app.load_config().unwrap();
    assert_eq!(app.config.get("from_env"), Some(&json!("loaded")));
    assert_eq!(app.config.get("mode_only"), Some(&json!("yes")));
    assert_eq!(app.config.get("order"), Some(&json!("plugin")));
    assert_eq!(app.config.get("plugin_value"), Some(&json!(42)));
    assert_eq!(app.config.version, "2.5.1");
    assert_eq!(app.config.get("version"), Some(&json!("2.5.1")));

    std::env::remove_var(env_name);
    std::fs::remove_dir_all(root).ok();
}

#[test]
fn exposes_total_configuration_defaults_and_special_transformations() {
    let mut config = Config::default();
    assert_eq!(config.get("$httpcompress"), Some(&json!(true)));
    assert_eq!(config.get("$wsmaxsize"), Some(&json!(256)));
    assert_eq!(config.get("$cookiesamesite"), Some(&json!("Lax")));
    assert!(config.get("$httpfiles").unwrap()["webmanifest"] == json!(true));

    config.set("$root", "/shop/").unwrap();
    config.set("$api", "/api/").unwrap();
    config.set("$httpfiles", "avif,csv").unwrap();
    config.set("totalapi", "a-long-total-api-secret").unwrap();
    config.set("$cryptoiv", "68656c6c6f").unwrap();
    config.set("mail_smtp", "smtp.example.com").unwrap();
    config
        .set("mail_smtp_options", json!({ "port": 2525, "secure": true }))
        .unwrap();
    assert_eq!(config.get("$root"), Some(&json!("/shop")));
    assert_eq!(config.get("$api"), Some(&json!("/shop/api/")));
    assert_eq!(config.get("$httpfiles").unwrap()["avif"], json!(true));
    assert_eq!(
        config.get("secret_totalapi"),
        Some(&json!("a-long-total-api-secret"))
    );
    assert_eq!(config.bytes("$cryptoiv"), Some(b"hello".as_slice()));
    assert_eq!(
        config.get("smtp").unwrap()["server"],
        json!("smtp.example.com")
    );
    assert_eq!(config.get("smtp").unwrap()["port"], json!(2525));
}

#[test]
fn configured_directories_and_reconfigure_hooks_are_applied() {
    let root = unique_temp_dir();
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("config"), "$dirpublic : web\nname : First\n").unwrap();
    let called = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let hook_called = called.clone();
    let mut app = Total::new().root(&root);
    app.load_config().unwrap();
    assert_eq!(app.paths.public(None), root.join("web"));
    app.on_reconfigure(move |app| {
        hook_called.store(true, std::sync::atomic::Ordering::SeqCst);
        assert_eq!(app.config.name, "Second");
        Ok(())
    });
    std::fs::write(root.join("config"), "$dirpublic : assets\nname : Second\n").unwrap();
    app.reload_config().unwrap();
    assert_eq!(app.paths.public(None), root.join("assets"));
    assert!(called.load(std::sync::atomic::Ordering::SeqCst));
    std::fs::remove_dir_all(root).ok();
}

#[test]
fn convention_loads_config_and_prepares_directories() {
    let root = unique_temp_dir();
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("config"), "name : Convention app\nport : 5099\n").unwrap();
    let previous_port = std::env::var("PORT").ok();
    std::env::set_var("PORT", "7777");

    let mut app = Total::new().root(&root);
    app.load_config().unwrap();
    app.prepare_directories().unwrap();

    assert_eq!(app.config.name, "Convention app");
    assert_eq!(app.config.port, 5099);
    assert!(root.join("logs").is_dir());
    assert!(root.join("tmp").is_dir());
    assert!(root.join("public").is_dir());
    if let Some(previous_port) = previous_port {
        std::env::set_var("PORT", previous_port);
    } else {
        std::env::remove_var("PORT");
    }
    std::fs::remove_dir_all(root).ok();
}

#[tokio::test]
async fn flowstream_rpc_is_callable_from_context() {
    let mut app = Total::new();
    app.flowstream("catalog").rpc("summary", |msg| async move {
        Ok(json!({
            "stream": msg.stream,
            "rpc": msg.name,
            "count": msg.get("count").and_then(Value::as_i64).unwrap_or(0)
        }))
    });
    app.route("GET /flow/", |ctx| async move {
        match ctx
            .flow_rpc("catalog", "summary", json!({ "count": 3 }))
            .await
        {
            Ok(value) => Response::success(value),
            Err(err) => err.into_response(),
        }
    })
    .unwrap();

    let response = app
        .router()
        .oneshot(
            Request::builder()
                .uri("/flow/")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let bytes = body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let payload: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(payload["value"]["count"], 3);
}

#[test]
fn build_discovery_generates_install_order_from_project_structure() {
    let root = unique_temp_dir();
    let src = root.join("src");
    std::fs::create_dir_all(src.join("schemas")).unwrap();
    std::fs::create_dir_all(src.join("controllers")).unwrap();
    std::fs::create_dir_all(src.join("services")).unwrap();
    std::fs::create_dir_all(src.join("flowstreams")).unwrap();
    std::fs::create_dir_all(src.join("plugins/jobs")).unwrap();
    std::fs::write(
            src.join("services/catalog.rs"),
            "total5::INSTALL! { NEWACTION!(\"Catalog/list\", |_ctx, _input| async move { Ok(serde_json::json!([])) }); }",
        )
        .unwrap();
    std::fs::write(
            src.join("flowstreams/catalog.rs"),
            "total5::INSTALL! { FLOWSTREAM!(\"catalog\", { FLOW_RPC!(\"summary\", |_msg| async move { Ok(serde_json::json!({})) }); }); }",
        )
        .unwrap();
    std::fs::write(
        src.join("schemas/products.rs"),
        "pub fn install(_: &mut total5::Total) -> Result<(), total5::Error> { Ok(()) }",
    )
    .unwrap();
    std::fs::write(
        src.join("controllers/products.rs"),
        "pub fn install(_: &mut total5::Total) -> Result<(), total5::Error> { Ok(()) }",
    )
    .unwrap();
    std::fs::write(
        src.join("plugins/jobs/mod.rs"),
        "pub fn install(_: &mut total5::Total) -> Result<(), total5::Error> { Ok(()) }",
    )
    .unwrap();

    let generated = build::discover_source(&root).unwrap();
    let service = generated.find("services_catalog::install").unwrap();
    let flowstream = generated.find("flowstreams_catalog::install").unwrap();
    let schema = generated.find("schemas_products::install").unwrap();
    let controller = generated.find("controllers_products::install").unwrap();
    let plugin = generated.find("plugins_jobs_mod::install").unwrap();

    assert!(service < flowstream);
    assert!(flowstream < schema);
    assert!(schema < controller);
    assert!(controller < plugin);
    std::fs::remove_dir_all(root).ok();
}

#[test]
fn db_builder_generates_common_sql() {
    let db = Db::new();
    assert_eq!(
        db.find("tbl_product")
            .fields("id,name")
            .r#where("isremoved", false)
            .sort("dtcreated", true)
            .limit(10)
            .sql(),
        "SELECT id,name FROM tbl_product WHERE isremoved=false ORDER BY dtcreated DESC LIMIT 10"
    );
    assert_eq!(
        db.update("tbl_product", json!({ "name": "A" }))
            .unwrap()
            .id("123")
            .sql(),
        "UPDATE tbl_product SET name='A' WHERE id='123'"
    );
}

#[test]
fn total_style_prototype_traits_work() {
    assert_eq!("Hello Total.rs".slug(), "hello-total-rs");
    assert!("a@b.com".is_email());
    assert!("https://example.com".is_url());
    assert_eq!("peter sirka".capitalize_total(), "Peter Sirka");
    assert_eq!(12.345_f64.floor_dec(2), 12.34);
    assert_eq!(12_i64.vat(20.0, false), 2.4);
    assert_eq!(vec![1, 2, 3].take_total(2), vec![1, 2]);
}

#[tokio::test]
async fn data_nosql_and_filestorage_work() {
    let root = unique_temp_dir();
    let data = Data::new(root.join("databases"));
    data.insert("nosql/users", json!({ "id": "1", "name": "Peter" }))
        .await
        .unwrap();
    assert_eq!(
        data.read("nosql/users", "1").await.unwrap().unwrap()["name"],
        "Peter"
    );
    data.update("nosql/users", "1", json!({ "name": "Total" }))
        .await
        .unwrap();
    assert_eq!(
        data.read("nosql/users", "1").await.unwrap().unwrap()["name"],
        "Total"
    );

    let storage = FileStorage::new(root.join("files"));
    let file = storage
        .save("hello.txt", b"hello", "text/plain")
        .await
        .unwrap();
    assert_eq!(storage.stat(&file.id).await.unwrap(), 5);
    assert_eq!(storage.read(&file.id).await.unwrap(), b"hello");
    storage.remove(&file.id).await.unwrap();
    std::fs::remove_dir_all(root).ok();
}

#[tokio::test]
async fn auth_hook_enriches_request_context() {
    let mut app = Total::new();
    app.auth(|mut ctx| async move {
        ctx.set("auth", "ok").unwrap();
        Ok(ctx)
    });
    app.route("GET /auth/", |ctx| async move {
        Response::success(json!({ "auth": ctx.get("auth") }))
    })
    .unwrap();

    let response = app
        .router()
        .oneshot(
            Request::builder()
                .uri("/auth/")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let bytes = body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let payload: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(payload["value"]["auth"], "ok");
}

#[tokio::test]
async fn controller_context_carries_user_model_and_route_auth() {
    let mut app = Total::new();
    app.auth(|mut ctx| async move {
        if ctx
            .headers
            .get("authorization")
            .and_then(|value| value.to_str().ok())
            == Some("Bearer test")
        {
            ctx.set_user(json!({ "id": "tester" })).unwrap();
        }
        Ok(ctx)
    });
    app.action("Account/read", |ctx, _input| async move {
        Ok(json!({
            "user": ctx.user(),
            "model": ctx.model(),
            "url": ctx.url(),
            "ua": ctx.ua()
        }))
    });
    app.route_compat("+GET /account/ --> Account/read").unwrap();
    app.route("-GET /signin/", |ctx| async move {
        ctx.success(json!({ "guest": !ctx.is_authenticated() }))
    })
    .unwrap();

    let router = app.router();
    let unauthorized = router
        .clone()
        .oneshot(
            Request::builder()
                .uri("/account/")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(unauthorized.status(), StatusCode::UNAUTHORIZED);

    let response = router
        .clone()
        .oneshot(
            Request::builder()
                .uri("/account/")
                .header("authorization", "Bearer test")
                .header("user-agent", "Total.rs test")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let bytes = body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let payload: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(payload["value"]["user"]["id"], "tester");
    assert_eq!(payload["value"]["model"], json!({}));
    assert_eq!(payload["value"]["url"], "/account/");
    assert_eq!(payload["value"]["ua"], "Total.rs test");

    let member_on_guest_route = router
        .oneshot(
            Request::builder()
                .uri("/signin/")
                .header("authorization", "Bearer test")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(member_on_guest_route.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn convention_installation_serves_all_testapp_routes() {
    let mut app = Total::new();
    app.schema("Products", |schema| {
        schema.action("smart_query", |_ctx, _input| async move {
            Ok(json!([{ "id": "1" }, { "id": "2" }]))
        });
        schema.action_options(
            "read",
            ActionOptions::new().params("*id:String").unwrap(),
            |_ctx, input| async move { Ok(json!({ "id": input["id"] })) },
        );
        Ok(())
    })
    .unwrap();
    app.route("GET /", |_ctx| async { Response::text("Total.rs") })
        .unwrap();
    app.route_compat("GET /products/ --> Products/smart_query")
        .unwrap();
    app.route_compat("GET /products/{id}/ --> Products/read")
        .unwrap();

    let router = app.router();
    for path in [
        "/",
        "/products/",
        "/products",
        "/products/123/",
        "/products/123",
    ] {
        let response = router
            .clone()
            .oneshot(Request::builder().uri(path).body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK, "route {path}");
    }
}

fn unique_temp_dir() -> PathBuf {
    std::env::temp_dir().join(format!("total5-test-{}", uuid::Uuid::new_v4().simple()))
}
