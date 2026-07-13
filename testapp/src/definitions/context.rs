total5::INSTALL! {
    CONF!(name = "Total.rs convention test app");
    CONF!(port = 5500);

    MIDDLEWARE!(|mut ctx| async move {
        ctx.set("request_source", "testapp")?;
        Ok(ctx)
    });

    AUTH!(|mut ctx| async move {
        if ctx
            .headers
            .get("authorization")
            .and_then(|value| value.to_str().ok())
            == Some("Bearer test")
        {
            ctx.set_user(serde_json::json!({ "id": "tester", "sa": true }))?;
        }
        Ok(ctx)
    });
}
