total5::INSTALL! {
    CONF!("reference_app", "totaljs-rust-proof");

    AUTH!(|mut ctx| async move {
        ctx.set("auth", "ok")?;
        Ok(ctx)
    });

    MIDDLEWARE!(|mut ctx| async move {
        ctx.set("request_source", "proofapp")?;
        Ok(ctx)
    });

    ON!("ready", |app| {
        println!(
            "proofapp ready: {} routes, {} actions, {} plugins, {} flowstreams",
            app.route_count(),
            app.action_count(),
            app.plugin_count(),
            app.flowstream_count()
        );
        Ok(())
    });
}
