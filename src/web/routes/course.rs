#![allow(non_camel_case_types)]

use std::sync::Arc;

use axum::{extract::Query, http::{HeaderMap, HeaderValue}, response::Response, routing::get, Router};
use reqwest::{cookie::Jar, header::{AUTHORIZATION, HOST}, Client, Url};
use serde::Deserialize;


use crate::{parsers::{self, available_parsers}, util::generic_firefox_headers};

use crate::errors::ErrorResponse;

pub fn routes() -> Router {
    Router::new()
        .route("/api/course", get(api_course_get_handler))
}

#[derive(Deserialize)]
struct CourseQuery {
    id: Option<String>,
    parser: Option<String>
}

// moodle_session_cookie string in a format MoodleSession=XXXXX
async fn get_course_html(moodle_session_cookie: String, course_id: String) -> Result<String, ErrorResponse> {
    let jar: Arc<Jar> = Jar::default().into();

    jar.add_cookie_str(&moodle_session_cookie, &"https://ekursy.put.poznan.pl".parse::<Url>().unwrap());

    let client = Client::builder()
        .cookie_provider(jar)
        .default_headers(generic_firefox_headers())
        .build()
        .unwrap();

    let url = format!("https://ekursy.put.poznan.pl/course/view.php?id={}", course_id);

    let Ok(resp) = client
        .get(&url)
        .header(HOST, HeaderValue::from_static("ekursy.put.poznan.pl"))
        .send()
        .await
        else { return Err(ErrorResponse::REMOTE_SERVER_DIDNT_RESPOND("ekursy course".into())) };
    
    if ! resp.url().to_string().starts_with(url.as_str()) {
        return Err(ErrorResponse::AUTH_FAILED("Session expired".into()))
    }
    
    let Ok(text) = resp.text().await
        else { return Err(ErrorResponse::REMOTE_SERVER_SENT_INVALID_DATA("ekursy course response couldnt be parsed".into())) };

    Ok(text)
}

async fn api_course_get_handler(headers: HeaderMap, Query(query): Query<CourseQuery>) -> Response {

    let parser_name = match query.parser {
        Some(p) => p,
        None => parsers::mercury::NAME.into()
    };

    if available_parsers().keys().find(|&&x| x == parser_name).is_none() {
        return ErrorResponse::BAD_REQUEST(format!("Parser with name \"{}\" doesn't exist", parser_name).into()).into_json_response();
    }

    let Some(moodle_session_id) = headers.get(AUTHORIZATION)
        else { return ErrorResponse::AUTH_FAILED("Authorization header with moodle session missing".into()).into_json_response(); };

    let moodle_session_cookie = String::from("MoodleSession=") + moodle_session_id.to_str().unwrap();

    let Some(course_id) = query.id
        else { return ErrorResponse::BAD_REQUEST("id query param missing".into()).into_json_response(); };
    

    let html = match get_course_html(moodle_session_cookie, course_id).await {
        Ok(h) => h,
        Err(r) => return r.into_json_response(),
    };

    available_parsers()[parser_name.as_str()].as_ref().parse(html)
}
