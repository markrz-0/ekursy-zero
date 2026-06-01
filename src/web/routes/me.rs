use std::{collections::HashMap, sync::Arc};

use axum::{Json, Router, http::{HeaderMap, HeaderValue}, response::{IntoResponse, Response}, routing::get};
use reqwest::{cookie::Jar, header::{AUTHORIZATION, HOST}, Client, Url};
use scraper::Selector;

use crate::{errors::ErrorResponse, util::generic_firefox_headers};

pub fn routes() -> Router {
    Router::new()
        .route("/api/me", get(me_info_handler))
}

// moodle_session_cookie string in a format MoodleSession=XXXXX
async fn me_info(moodle_session_cookie: String) -> Result<Response, ErrorResponse> {
    let jar: Arc<Jar> = Jar::default().into();

    jar.add_cookie_str(&moodle_session_cookie, &"https://ekursy.put.poznan.pl".parse::<Url>().unwrap());

    let client = Client::builder()
        .cookie_provider(jar)
        .default_headers(generic_firefox_headers())
        .build()
        .unwrap();

    let url = format!("https://ekursy.put.poznan.pl/user/profile.php");

    let Ok(resp) = client
        .get(&url)
        .header(HOST, HeaderValue::from_static("ekursy.put.poznan.pl"))
        .send()
        .await
        else { return Err(ErrorResponse::REMOTE_SERVER_DIDNT_RESPOND("ekursy resource".into())) };
    
    if resp.url().to_string().starts_with("https://ekursy.put.poznan.pl/login/index.php") {
        return Err(ErrorResponse::AUTH_FAILED("Session expired".into()))
    }

    let Ok(text) = resp.text().await
        else { return Err(ErrorResponse::REMOTE_SERVER_SENT_INVALID_DATA("ekursy course response couldnt be parsed".into())) };

    let document = scraper::Html::parse_document(text.as_str());

    let node_selector = Selector::parse(".contentnode").unwrap();
    let dt_selector = Selector::parse("dt").unwrap();
    let dd_selector = Selector::parse("dd").unwrap();

    // A standard HashMap automatically serializes to valid JSON in Axum
    let mut results: HashMap<String, String> = HashMap::new();

    // Iterate over EVERY matching .contentnode (dropping the .next())
    for node in document.select(&node_selector) {
        
        // Extract all dt inner texts within this specific node
        let keys = node.select(&dt_selector)
            .map(|dt| dt.text().collect::<String>().trim().to_string());
            
        // Extract all dd inner texts within this specific node
        let values = node.select(&dd_selector)
            .map(|dd| dd.text().collect::<String>().trim().to_string());

        // Zip them together to create key-value pairs and insert into the map
        for (key, value) in keys.zip(values) {
            results.insert(key, value);
        }
    }
    
    Ok(Json(results).into_response())
}

async fn me_info_handler(headers: HeaderMap) -> Response {
    let Some(moodle_session_id) = headers.get(AUTHORIZATION)
        else { return ErrorResponse::AUTH_FAILED("Authorization header with moodle session missing".into()).into_json_response(); };

    let moodle_session_cookie = String::from("MoodleSession=") + moodle_session_id.to_str().unwrap();

    match me_info(moodle_session_cookie).await {
        Ok(r) => return r,
        Err(e ) => return e.into_json_response()
    }
}