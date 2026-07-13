use serde_json::json;

total5::INSTALL! {
    FLOWSTREAM!("catalog", {
        FLOW_INPUT!("audit", |msg| async move {
            Ok(json!({
                "accepted": true,
                "stream": msg.stream,
                "input": msg.name,
                "data": msg.data
            }))
        });

        FLOW_RPC!("summary", |msg| async move {
            Ok(json!({
                "stream": msg.stream,
                "rpc": msg.name,
                "count": msg.get("count").and_then(|value| value.as_i64()).unwrap_or(0)
            }))
        });
    });
}
