use serde_json::json;
use total5::Plugin;

total5::INSTALL! {
    PLUGIN!(
        "audit",
        Plugin::new("Audit trail")
            .icon("ti ti-history")
            .group("System")
            .position(10)
            .permission("audit_read", "Read audit events")
    );

    ROUTE!("GET /audit/events/", audit_events);
}

async fn audit_events(_ctx: total5::Context) -> total5::Response {
    total5::Response::success(json!([
        { "id": "evt-1", "message": "plugin discovered" },
        { "id": "evt-2", "message": "route registered by plugin" }
    ]))
}
