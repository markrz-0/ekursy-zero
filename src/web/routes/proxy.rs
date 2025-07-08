use std::sync::Arc;

use axum::{extract::Query, http::{HeaderMap, HeaderValue}, response::{IntoResponse, Response}, routing::get, Router};
use reqwest::{cookie::Jar, header::{AUTHORIZATION, CONTENT_TYPE, HOST}, Client, Url};
use serde::Deserialize;

use crate::{errors::ErrorResponse, util::generic_firefox_headers};

pub fn routes() -> Router {
    Router::new()
        .route("/api/proxy", get(pluginfile_handler))
}


#[derive(Debug, Deserialize)]
struct ProxyQuery {
    path: Option<String>
}


// moodle_session_cookie string in a format MoodleSession=XXXXX
async fn proxy(moodle_session_cookie: String, path: String) -> Result<Response, ErrorResponse> {
    let jar: Arc<Jar> = Jar::default().into();

    jar.add_cookie_str(&moodle_session_cookie, &"https://ekursy.put.poznan.pl".parse::<Url>().unwrap());

    let client = Client::builder()
        .cookie_provider(jar)
        .default_headers(generic_firefox_headers())
        .build()
        .unwrap();

    let url = format!("https://ekursy.put.poznan.pl/{}", path);

    let Ok(resp) = client
        .get(&url)
        .header(HOST, HeaderValue::from_static("ekursy.put.poznan.pl"))
        .send()
        .await
        else { return Err(ErrorResponse::REMOTE_SERVER_DIDNT_RESPOND("ekursy resource".into())) };
    
    if resp.url().to_string().starts_with("https://ekursy.put.poznan.pl/login/index.php") {
        return Err(ErrorResponse::AUTH_FAILED("Session expired".into()))
    }

    let Some(content_type_borrowed) = resp.headers().get(CONTENT_TYPE)
        else { return Err(ErrorResponse::REMOTE_SERVER_SENT_INVALID_DATA("No Content-Type header".into())) };
    let content_type = content_type_borrowed.to_owned();

    let Ok(bytes) = resp.bytes().await
        else { return Err(ErrorResponse::UNABLE_TO_PARSE_RESPONSE_TEXT("Bytes not recieved from remote WTF?".into())) };

    let headers = [
        (CONTENT_TYPE, content_type)
    ];

    return Ok((headers, bytes).into_response())

}

async fn pluginfile_handler(headers: HeaderMap, Query(query): Query<ProxyQuery>) -> Response {
    let Some(moodle_session_id) = headers.get(AUTHORIZATION)
        else { return ErrorResponse::AUTH_FAILED("Authorization header with moodle session missing".into()).into_json_response(); };

    let moodle_session_cookie = String::from("MoodleSession=") + moodle_session_id.to_str().unwrap();

    let Some(path) = query.path
        else { return ErrorResponse::BAD_REQUEST("path query param missing".into()).into_json_response(); };

    match proxy(moodle_session_cookie, path).await {
        Ok(r) => return r,
        Err(e ) => return e.into_json_response()
    }
}