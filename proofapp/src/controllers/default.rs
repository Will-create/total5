use serde_json::json;
use total5::{Context, Response};

total5::INSTALL! {
    ROUTE!("GET /", index);
    ROUTE!("GET /status/", status);
    ROUTE!("GET /catalog/ --> Catalog/list");
    ROUTE!("GET /catalog/health/ --> Catalog/health");
    ROUTE!("GET /products/ --> Products/query");
    ROUTE!("GET /products/{id}/ --> Products/read");
    ROUTE!("GET /flow/catalog/summary/", flow_summary);
    ROUTE!("GET /data/demo/", data_demo);
    ROUTE!("GET /files/demo/", files_demo);
    WEBSOCKET!("/ws/", websocket);
}

async fn index(ctx: Context) -> Response {
    Response::success(json!({
        "name": ctx.config().name,
        "mode": ctx.config().string("mode"),
        "reference_app": ctx.config().string("reference_app"),
        "auth": ctx.get("auth"),
        "request_source": ctx.get("request_source")
    }))
}

async fn status(ctx: Context) -> Response {
    Response::success(json!({
        "ok": true,
        "plugins": ctx.plugins().keys().cloned().collect::<Vec<_>>()
    }))
}

async fn flow_summary(ctx: Context) -> Response {
    match ctx.flow_rpc("catalog", "summary", json!({ "count": 2 })).await {
        Ok(value) => Response::success(value),
        Err(err) => err.into_response(),
    }
}

async fn data_demo(ctx: Context) -> Response {
    let id = "proof-doc";
    let _ = ctx.data().remove("proof/docs", id).await;
    if let Err(err) = ctx
        .data()
        .insert("proof/docs", json!({ "id": id, "name": "NoSQL proof" }))
        .await
    {
        return err.into_response();
    }
    match ctx.data().read("proof/docs", id).await {
        Ok(value) => Response::success(value),
        Err(err) => err.into_response(),
    }
}

async fn files_demo(ctx: Context) -> Response {
    match ctx
        .filestorage()
        .save("proof.txt", b"filestorage proof", "text/plain")
        .await
    {
        Ok(file) => Response::success(json!({ "id": file.id, "name": file.name, "size": file.size })),
        Err(err) => err.into_response(),
    }
}

async fn websocket(mut ws: total5::WsContext) {
    let _ = ws.send_json(json!({ "type": "hello", "framework": "total5" })).await;
}
