use std::net::SocketAddr;

use axum::{extract::{ConnectInfo, Request}, middleware::Next, response::Response};


pub async fn access_logger_middleware(
    ConnectInfo(connect_info): ConnectInfo<SocketAddr>,
    req: Request,
    next: Next
) -> Response {
    let path_and_query = match req.uri().path_and_query() {
        Some(p) => &p.to_string(),
        None => "/"
    };
    let method = req.method().to_string();
    let ip = connect_info.ip().to_string();

    let resp = next.run(req).await;

    let status_code = resp.status().as_u16().to_string();

    println!("{:<16} {:<4} - {:<6} {}", ip, status_code, method, path_and_query);

    resp
}