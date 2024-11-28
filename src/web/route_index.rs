use axum::{body::Body, extract::Query, http::{header, StatusCode}, response::{IntoResponse, Response}, routing::get, Router};
use serde::Deserialize;
use tokio_util::io::ReaderStream;

use super::errors::ErrorResponse;

pub fn routes() -> Router {
    Router::new()
        .route("/",  get(index_handler))
}

#[derive(Debug, Deserialize)]
struct IndexQuery {
    invalid: Option<String>
}

async fn index_handler(Query(query): Query<IndexQuery>) -> Response {
    let headers = [
        (header::CONTENT_TYPE, "text/html")
    ];

    let mut file_path = "templates/index.html";

    if query.invalid.is_some() {
        file_path = "templates/index-invalid.html";
    }

    let file = match tokio::fs::File::open(file_path).await {
        Ok(file) => file,
        Err(err) => return ErrorResponse::TEMPLATE_FILE_ERROR(format!("File not found: {}", err)).into_response(),
    };

    let stream = ReaderStream::new(file);

    let body = Body::from_stream(stream);

    (StatusCode::OK, headers, body).into_response()
}