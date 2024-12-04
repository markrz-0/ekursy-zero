use std::sync::Arc;

use axum::{http::{HeaderMap, HeaderValue}, response::{Html, IntoResponse, Response}, routing::get, Json, Router};
use reqwest::{cookie::Jar, header::{AUTHORIZATION, HOST}, Client, Url};
use scraper::Selector;
use serde::Serialize;
use serde_json::json;
use tokio::io::AsyncReadExt;
use tower_cookies::Cookies;

use super::{errors::ErrorResponse, generic_firefox_headers, to_safe_html};

pub fn routes() -> Router {
    Router::new()
        .route("/dashboard", get(dashboard_get_handler))
        .route("/api/dashboard", get(api_dashboard_get_handler))
}


#[derive(Debug, Serialize)]
struct Course {
    name: String,
    id: String
}

// moodle_session_cookie string in a format MoodleSession=XXXXX
async fn get_course_list(moodle_session_cookie: String) -> Result<Vec<Course>, ErrorResponse> {

    let jar: Arc<Jar> = Jar::default().into();

    jar.add_cookie_str(&moodle_session_cookie, &"https://ekursy.put.poznan.pl".parse::<Url>().unwrap());

    let client = Client::builder()
        .cookie_provider(jar)
        .default_headers(generic_firefox_headers())
        .build()
        .unwrap();

    let Ok(resp) = client
        .get("https://ekursy.put.poznan.pl/")
        .header(HOST, HeaderValue::from_static("ekursy.put.poznan.pl"))
        .send()
        .await
        else { return Err(ErrorResponse::REMOTE_SERVER_DIDNT_RESPOND("ekursy dashboard".into())) };

    let Ok(text) = resp.text().await
        else { return Err(ErrorResponse::REMOTE_SERVER_SENT_INVALID_DATA("ekursy dashboard response couldnt be parsed".into())) };

    let document = scraper::Html::parse_document(text.as_str());
    let selector = Selector::parse(".dropdown-menu").unwrap();
    let Some(element) = document.select(&selector).next()
        else { return Err(ErrorResponse::AUTH_FAILED("Session expired".into())) }; // technically it is invalid data but such response is mainly expected if session expired

    let mut out = vec![];

    for course_item in element.child_elements() {
        let href_option = course_item.value().attr("href");

        if href_option.is_none() {
            continue;
        }

        let href = href_option.unwrap();
        
        if !href.starts_with("https://ekursy.put.poznan.pl/course/view.php?id=") {
            continue;
        }

        let course_id = href.replace("https://ekursy.put.poznan.pl/course/view.php?id=", "");

        let mut text = String::new();
        for text_node in course_item.text() {
            text += text_node;
        }

        out.push(Course {
            name: text,
            id: course_id
        });
    }

    out.sort_by(|a, b| a.name.cmp(&b.name));

    Ok(out)
}

async fn api_dashboard_get_handler(headers: HeaderMap) -> Response {
    let Some(moodle_session_id) = headers.get(AUTHORIZATION)
        else { return ErrorResponse::AUTH_FAILED("Authorization header with moodle session missing".into()).into_json_response(); };

    let moodle_session_cookie = String::from("MoodleSession=") + moodle_session_id.to_str().unwrap();

    let courses = match get_course_list(moodle_session_cookie).await {
        Ok(c) => c,
        Err(r) => return r.into_json_response()
    };

    Json(json!({
        "msg": "OK",
        "courses": courses
    })).into_response()
}

async fn dashboard_get_handler(cookies: Cookies) -> Response {
    let Some(moodle_session_id) = cookies.get("MoodleSession")
        else { return ErrorResponse::AUTH_FAILED("Moodle session cookie missing".into()).into_response(); };


    let moodle_session_cookie = moodle_session_id.to_string();

    let courses = match get_course_list(moodle_session_cookie).await {
        Ok(c) => c,
        Err(r) => return r.into_response()
    };

    let mut output_html = String::from("<a href=\"/logout\">log out</a><br/><br/>");
    for course in courses {
        output_html += format!(
                "<a href=\"/course?id={}\">{}</a><br/>",
                urlencoding::encode(course.id.as_str()),
                to_safe_html(&course.name)
            ).as_str();
    }

    let mut file = match tokio::fs::File::open("templates/content.html").await {
        Ok(file) => file,
        Err(err) => return ErrorResponse::TEMPLATE_FILE_ERROR(err.to_string()).into_response(),
    };

    let mut text = String::new();
    if let Err(e) = file.read_to_string(&mut text).await {
        return ErrorResponse::TEMPLATE_FILE_ERROR(e.to_string()).into_response()
    }

    Html(text.replace("{% CONTENT %}", output_html.as_str())).into_response()
}