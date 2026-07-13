use serde_json::json;

total5::INSTALL! {
    SCHEMA!("Products", {
        ACTION!("query", |_ctx, _input| async move {
            Ok(json!([
                { "id": "1", "name": "Convention-first API", "stock": 8 },
                { "id": "2", "name": "Discovered controller", "stock": 5 }
            ]))
        });

        ACTION!(
            "read",
            params: "*id:String",
            |_ctx, input| async move {
                Ok(json!({
                    "id": input["id"],
                    "name": format!("Product {}", input["id"].as_str().unwrap_or("")),
                    "source": "schema-action"
                }))
            }
        );
    });
}
