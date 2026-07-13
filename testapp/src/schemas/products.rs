use serde_json::json;

total5::INSTALL! {
    SCHEMA!("Products", {
        ACTION!("smart_query", |_ctx, _input| async move {
            Ok(json!([
                { "id": "1", "name": "First product" },
                { "id": "2", "name": "Second product" }
            ]))
        });
        ACTION!(
            "read",
            params: "*id:String",
            |_ctx, input| async move {
                Ok(json!({
                    "id": input["id"],
                    "name": "Product detail"
                }))
            }
        );
        ACTION!("context", |ctx, _input| async move {
            Ok(json!({
                "user": ctx.user(),
                "model": ctx.model(),
                "url": ctx.url(),
                "ua": ctx.ua()
            }))
        });
    });
}
