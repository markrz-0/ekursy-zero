#![allow(non_camel_case_types)]

use std::sync::Arc;

use axum::{extract::Query, http::{HeaderMap, HeaderValue}, response::{Html, IntoResponse, Response}, routing::get, Json, Router};
use reqwest::{cookie::Jar, header::{AUTHORIZATION, HOST}, Client, Url};
use scraper::{selectable::Selectable, ElementRef, Selector};
use serde::{Deserialize, Serialize};
use serde_json::json;
use tokio::io::AsyncReadExt;
use tower_cookies::Cookies;

use crate::web::generic_firefox_headers;

use super::errors::{ErrorResponse, IntoJsonResponse};

pub fn routes() -> Router {
    Router::new()
        .route("/course", get(course_get_handler))
        .route("/api/course", get(api_course_get_handler))
}

#[derive(Deserialize)]
struct ViewQuery {
    id: Option<String>
}

#[derive(Debug, Serialize)]
enum ResourceKind {
    TITLE,
    DESCRIPTION,
    CARD_TITLE,
    CARD_DESCRIPTION,
    RESOURCE_DESCRIPTION,
    FILE,
    URL,
    QUIZ,
    OTHER,
    INVALID
}

#[derive(Debug, Serialize)]
struct Resource {
    kind: ResourceKind,
    data: Vec<String>,
    id: Option<String>
}

impl Resource {
    fn into_html_string(&self) -> String {
        let output_html ;
        
        if let Some(id) = &self.id {
            output_html = format!("<p><a target=\"_blank\" href=\"/view?id={}\">{}</a></p>", id, self.data.join(""));
            return output_html;
        }

        output_html = match self.kind {
            ResourceKind::TITLE => format!("<h1>{}</h1>", self.data.join("")),
            ResourceKind::CARD_TITLE => format!("<h2>{}</h2>", self.data.join("")),
            ResourceKind::DESCRIPTION | ResourceKind::RESOURCE_DESCRIPTION => format!("<p>{}</p>", self.data.join("<br/>")),
            _ => format!("<p>{}</p>", self.data.join(""))
        };

        output_html
    }
}


fn get_text<'a>(document: impl scraper::selectable::Selectable<'a>, selector: &Selector) -> Result<Vec<String>, ()>{
    let Some(element) = document.select(&selector).next()
        else { return Err(()) };

    let mut text = vec![];
    for text_node in element.text() {
        text.push(text_node.into());
    }

    Ok(text)
}

fn list_text_in_child_paragraphs<'a>(document: impl scraper::selectable::Selectable<'a>, selector: &Selector) -> Result<Vec<String>, ()> {
    let Some(element) = document.select(&selector).next()
        else { return Err(()) };
    
    let mut out = vec![];
    for p in element.select(&Selector::parse("p, li").unwrap()) {
        let mut text = String::new();
        for text_node in p.text(){
            text += text_node;
        }
        out.push(text);
    }

    Ok(out)
}

fn construct_resource<'a>(resource_element: ElementRef<'a>) -> Result<Resource, ErrorResponse> {
    let resource_kind;

    if resource_element.value().has_class("label", scraper::CaseSensitivity::AsciiCaseInsensitive) {
        resource_kind = ResourceKind::RESOURCE_DESCRIPTION;
    }
    else if resource_element.value().has_class("url", scraper::CaseSensitivity::AsciiCaseInsensitive) {
        resource_kind = ResourceKind::URL;
    }
    else if resource_element.value().has_class("quiz", scraper::CaseSensitivity::AsciiCaseInsensitive) {
        resource_kind = ResourceKind::QUIZ;
    }
    else if resource_element.value().has_class("resource", scraper::CaseSensitivity::AsciiCaseInsensitive) {
        resource_kind = ResourceKind::FILE;
    }
    else {
        resource_kind = ResourceKind::OTHER;
    }

    let text = match resource_kind {
        ResourceKind::RESOURCE_DESCRIPTION => list_text_in_child_paragraphs(resource_element, &Selector::parse(".contentwithoutlink").unwrap()),
        _ => get_text(resource_element, &Selector::parse(".activityinstance").unwrap())
    };

    let Ok(text_unwraped) = text
        else { return Err(ErrorResponse::REMOTE_SERVER_SENT_INVALID_DATA("invalid resource".into())) };

    let mut r = Resource {
        kind: resource_kind,
        data: text_unwraped,
        id: None,
    };

    if let Some(resource_link) = resource_element.select(&Selector::parse(".aalink").unwrap()).next() {
        if let Some(link) = resource_link.value().attr("href") {
            r.id = Some(link.replace("https://ekursy.put.poznan.pl/mod/", "").replace("/view.php?", "$"))
        }
    }

    Ok(r)
}

fn extract_resources<'a>(element: impl Selectable<'a>) -> Result<Vec<Resource>, ErrorResponse> {
    let mut out = vec![];
    let resource_selector = Selector::parse(".activity").unwrap();
    for resource in element.select(&resource_selector) {
        
        let r = match construct_resource(resource) {
            Ok(r) => r,
            Err(_) => Resource { kind: ResourceKind::INVALID, data: vec!["Invalid resource".into()], id: None }
        };

        out.push(r);
    }

    Ok(out)
}

// moodle_session_cookie string in a format MoodleSession=XXXXX
async fn get_resource_list(moodle_session_cookie: String, course_id: String) -> Result<Vec<Resource>, ErrorResponse> {
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

    let document = scraper::Html::parse_document(text.as_str());

    let mut out = vec![];

    // get title
    let Ok(h1_text) = get_text(&document, &Selector::parse("h1").unwrap())
        else { return Err(ErrorResponse::REMOTE_SERVER_SENT_INVALID_DATA("H1 not present".into())) };
    
    out.push(Resource { kind: ResourceKind::TITLE, data: h1_text, id: None });

    // description
    let Ok(desc) = list_text_in_child_paragraphs(&document, &Selector::parse(".summary:not(.card-text)").unwrap())
        else { return Err(ErrorResponse::REMOTE_SERVER_SENT_INVALID_DATA("desc not present".into())) };

    out.push(Resource { kind: ResourceKind::DESCRIPTION, data: desc, id: None });

    let cards_selector = Selector::parse(".card.section.main > div").unwrap();
    let cards = document.select(&cards_selector);
    for card in cards {
        let Ok(title) = get_text(card, &Selector::parse(".sectionname").unwrap())
            else { return Err(ErrorResponse::REMOTE_SERVER_SENT_INVALID_DATA("card title not present".into())) };
        out.push(Resource { kind: ResourceKind::CARD_TITLE, data: title, id: None });

        let Ok(desc) = get_text(card, &Selector::parse(".summary").unwrap())
            else { return Err(ErrorResponse::REMOTE_SERVER_SENT_INVALID_DATA("card desc not present".into())) };
        out.push(Resource { kind: ResourceKind::CARD_DESCRIPTION, data: desc, id: None });

        let resources = match extract_resources(card) {
            Ok(r) => r,
            Err(e) => return Err(e)
        };

        out.extend(resources);
    }

    let topics_selector = Selector::parse(".topics").unwrap();
    let mut topics = document.select(&topics_selector);
    let topics_container = topics.next();
    if topics_container.is_none() {
        return Ok(out)
    } 

    let sections_selector = Selector::parse(".section.main").unwrap();
    let sections = document.select(&sections_selector);
    for section in sections {
        let Ok(title) = get_text(section, &Selector::parse("h3").unwrap())
            else { return Err(ErrorResponse::REMOTE_SERVER_SENT_INVALID_DATA("title not present in section".into())) };
        out.push(Resource { kind: ResourceKind::CARD_TITLE, data: title, id: None });

        let resources = match extract_resources(section) {
            Ok(r) => r,
            Err(e) => return Err(e)
        };

        out.extend(resources);

    }


    Ok(out)
}

async fn api_course_get_handler(headers: HeaderMap, Query(query): Query<ViewQuery>) -> Response {
    let Some(moodle_session_id) = headers.get(AUTHORIZATION)
        else { return ErrorResponse::AUTH_FAILED("Authorization header with moodle session missing".into()).into_json_response(); };

    let moodle_session_cookie = String::from("MoodleSession=") + moodle_session_id.to_str().unwrap();

    let Some(course_id) = query.id
        else { return ErrorResponse::BAD_REQUEST("id query param missing".into()).into_json_response(); };
    

    let resources = match get_resource_list(moodle_session_cookie, course_id).await {
        Ok(r) => r,
        Err(r) => return r.into_json_response(),
    };

    Json(json!({
        "msg": "OK",
        "resources": resources
    })).into_response()
}

async fn course_get_handler(cookies: Cookies, Query(query): Query<ViewQuery>) -> Response  {
    let Some(moodle_session_id) = cookies.get("MoodleSession")
        else { return ErrorResponse::AUTH_FAILED("Moodle session cookie missing".into()).into_response(); };

    let moodle_session_cookie = moodle_session_id.to_string();

    let Some(course_id) = query.id
    else { return ErrorResponse::BAD_REQUEST("id query param missing".into()).into_response(); };


    let resources = match get_resource_list(moodle_session_cookie, course_id).await {
        Ok(r) => r,
        Err(r) => return r.into_response(),
    };

    let mut output_html = String::from("<a href=\"/logout\">log out</a><br/><br/>");
    for resource in resources {
        output_html += resource.into_html_string().as_str();
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