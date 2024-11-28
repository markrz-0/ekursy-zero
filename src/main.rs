mod web;
mod accesslogger;

use std::net::SocketAddr;

use axum::{middleware, Router};
use tower_cookies::CookieManagerLayer;


#[tokio::main]
async fn main() {

    let app = Router::new()
        .merge(web::routes())
        .layer(CookieManagerLayer::new())
        .layer(middleware::from_fn(accesslogger::access_logger_middleware))
        .into_make_service_with_connect_info::<SocketAddr>();

    let listener = tokio::net::TcpListener::bind("0.0.0.0:8080")
        .await
        .unwrap();
    println!("LISTENING on 0.0.0.0:8080");
    axum::serve(listener, app).await.unwrap();
}
