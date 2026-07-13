use serde_json::json;

total5::INSTALL! {
    NEWACTION!("Catalog/list", |_ctx, _input| async move {
        Ok(json!([
            { "id": "book", "name": "Total.js Handbook", "kind": "guide" },
            { "id": "starter", "name": "Rust Proof Starter", "kind": "template" }
        ]))
    });

    NEWACTION!("Catalog/health", |_ctx, _input| async move {
        Ok(json!({ "service": "catalog", "ok": true }))
    });
}
