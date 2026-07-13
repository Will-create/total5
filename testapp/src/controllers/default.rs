use total5::{Context, Response};

total5::INSTALL! {
    ROUTE!("GET /", index);
    ROUTE!("GET /products/ --> Products/smart_query");
    ROUTE!("GET /products/{id}/ --> Products/read");
    ROUTE!("API /api/ -products --> Products/smart_query");
    ROUTE!("API /api/ -products_read/{id} --> Products/read");
    ROUTE!("+API /api/ +products_context --> Products/context");
}

async fn index(_ctx: Context) -> Response {
    Response::text("Total.rs")
}
