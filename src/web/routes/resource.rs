use std::sync::Arc;

use axum::{extract::Query, http::{HeaderMap, HeaderValue}, response::{IntoResponse, Redirect, Response}, routing::get, Router};
use reqwest::{cookie::Jar, header::{AUTHORIZATION, CONTENT_TYPE, HOST}, Client, Url};
use scraper::Selector;
use serde::Deserialize;

use crate::{errors::ErrorResponse, util::generic_firefox_headers};

pub fn routes() -> Router {
    Router::new()
        .route("/api/resource", get(resource_handler))
}


#[derive(Debug, Deserialize)]
struct ResourceQuery {
    id: Option<String>,
    kind: Option<String>
}

async fn extract_resource(req_url: &str, resp: reqwest::Response, client: &Client) -> Result<Response, ErrorResponse> {
    // external file redirect
    if req_url != resp.url().to_string() {
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

    let Ok(text) = resp.text().await
        else { return Err(ErrorResponse::REMOTE_SERVER_SENT_INVALID_DATA("ekursy course response couldnt be parsed".into())) };

    // weird scopy thing
    // https://github.com/rust-lang/rust/issues/63768
    // https://users.rust-lang.org/t/future-is-not-send-as-this-value-is-used-across-an-await-but-i-drop-the-value-before-the-await/57574/5
    let iframe_src: Result<String, ()> = { 
        let document = scraper::Html::parse_document(text.as_str());
        let iframe = document.select(&Selector::parse("iframe").unwrap()).next();
        let mut out: Result<String, ()> = Err(());
        if iframe.is_some() {
            let src = iframe.unwrap().value().attr("src");
            if src.is_some() {
                out = Ok(src.unwrap().to_owned())
            }
        }
        // non-Send variables document and iframe are implicitly dropped and dont infect the function
        // dropping them explicitly doesnt work (as in linked issue)
        out
    };

    if iframe_src.is_ok() {
        let Ok(resp2) = client
            .get(&iframe_src.unwrap())
            .header(HOST, HeaderValue::from_static("ekursy.put.poznan.pl"))
            .send()
            .await
            else { return Err(ErrorResponse::REMOTE_SERVER_DIDNT_RESPOND("iframe resource".into())) };
        
        let Some(content_type_borrowed) = resp2.headers().get(CONTENT_TYPE)
            else { return Err(ErrorResponse::REMOTE_SERVER_SENT_INVALID_DATA("No Content-Type header".into())) };
        let content_type = content_type_borrowed.to_owned();

        let Ok(bytes) = resp2.bytes().await
            else { return Err(ErrorResponse::UNABLE_TO_PARSE_RESPONSE_TEXT("Invalid bytes. WTF?".into())) };
        

        let headers = [
            (CONTENT_TYPE, content_type)
        ];

        return Ok((headers, bytes).into_response())
    }

    // TODO: video test & other

    return Err(ErrorResponse::BAD_REQUEST("No parser for requested resource of type \"resource\" exists".into()));
}

async fn extract_url(resp: reqwest::Response) -> Result<Response, ErrorResponse> {
    
    let Ok(text) = resp.text().await
        else { return Err(ErrorResponse::REMOTE_SERVER_SENT_INVALID_DATA("ekursy course response couldnt be parsed".into())) };

    let document = scraper::Html::parse_document(text.as_str());
    
    let Some(urlworkaround_element) = document.select(&Selector::parse(".urlworkaround").unwrap()).next()
        else { return Err(ErrorResponse::REMOTE_SERVER_SENT_INVALID_DATA("Unable to select url wrapper in resource".into())) };

    let Some(link_element) = urlworkaround_element.select(&Selector::parse("a").unwrap()).next()
        else { return Err(ErrorResponse::REMOTE_SERVER_SENT_INVALID_DATA("Unable to select url element in resource".into())) };

    let Some(url) = link_element.value().attr("href")
        else { return Err(ErrorResponse::REMOTE_SERVER_SENT_INVALID_DATA("Unable to select url in resource".into())) };

    return Ok(Redirect::to(url).into_response());
}

// moodle_session_cookie string in a format MoodleSession=XXXXX
async fn fetch_resource(moodle_session_cookie: String, resource_id: String, resource_kind: String) -> Result<Response, ErrorResponse> {
    let jar: Arc<Jar> = Jar::default().into();

    jar.add_cookie_str(&moodle_session_cookie, &"https://ekursy.put.poznan.pl".parse::<Url>().unwrap());

    let client = Client::builder()
        .cookie_provider(jar)
        .default_headers(generic_firefox_headers())
        .build()
        .unwrap();

    let url = format!("https://ekursy.put.poznan.pl/mod/{}/view.php?id={}", resource_kind, resource_id);

    let Ok(resp) = client
        .get(&url)
        .header(HOST, HeaderValue::from_static("ekursy.put.poznan.pl"))
        .send()
        .await
        else { return Err(ErrorResponse::REMOTE_SERVER_DIDNT_RESPOND("ekursy resource".into())) };
    
    if resp.url().to_string().starts_with("https://ekursy.put.poznan.pl/login/index.php") {
        return Err(ErrorResponse::AUTH_FAILED("Session expired".into()))
    }

    match resource_kind.as_str() {
        "resource" => return extract_resource(&url, resp, &client).await,
        "url" => return extract_url(resp).await,
        _ => return Err(ErrorResponse::UNABLE_TO_PARSE_RESPONSE_TEXT("Unknown resource type".into()))
    } 
}

async fn resource_handler(headers: HeaderMap, Query(query): Query<ResourceQuery>) -> Response {
    let Some(moodle_session_id) = headers.get(AUTHORIZATION)
        else { return ErrorResponse::AUTH_FAILED("Authorization header with moodle session missing".into()).into_json_response(); };

    let moodle_session_cookie = String::from("MoodleSession=") + moodle_session_id.to_str().unwrap();

    let Some(resource_id) = query.id
        else { return ErrorResponse::BAD_REQUEST("id query param missing".into()).into_json_response(); };

    let Some(resource_kind) = query.kind
        else { return ErrorResponse::BAD_REQUEST("kind query param missing".into()).into_json_response(); };

    match fetch_resource(moodle_session_cookie, resource_id, resource_kind).await {
        Ok(r) => return r,
        Err(e ) => return e.into_json_response()
    }
}