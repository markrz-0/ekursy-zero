use std::sync::Arc;

use axum::{
    extract::Query,
    http::{HeaderMap, HeaderValue},
    response::{IntoResponse, Response},
    routing::get,
    Json, Router,
};
use reqwest::{
    cookie::Jar,
    header::{AUTHORIZATION, HOST},
    Client, Url,
};
use scraper::Selector;
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::{errors::ErrorResponse, util::generic_firefox_headers};

pub fn routes() -> Router {
    Router::new()
        .route("/api/forum", get(api_forum_get_handler))
        .route("/api/forum/discussion", get(api_discussion_get_handler))
}

#[derive(Deserialize)]
struct ForumQuery {
    id: Option<String>,
}

#[derive(Deserialize)]
struct DiscussionQuery {
    id: Option<String>,
    d: Option<String>,
}

#[derive(Debug, Serialize, Clone, PartialEq, Eq)]
pub struct DiscussionThread {
    pub id: String,
    pub title: String,
}

#[derive(Debug, Serialize, Clone, PartialEq, Eq)]
pub struct ForumDetails {
    pub id: String,
    pub title: String,
    pub description: String,
    pub threads: Vec<DiscussionThread>,
}

#[derive(Debug, Serialize, Clone, PartialEq, Eq)]
pub struct ForumPost {
    pub id: String,
    pub subject: String,
    pub author: String,
    pub time: String,
    pub content: String,
}

#[derive(Debug, Serialize, Clone, PartialEq, Eq)]
pub struct DiscussionDetails {
    pub id: String,
    pub title: String,
    pub posts: Vec<ForumPost>,
}

// Custom parser to extract forum details and threads
pub fn parse_forum_html(html: &str, forum_id: String) -> Result<ForumDetails, ErrorResponse> {
    let document = scraper::Html::parse_document(html);

    let title_selector = Selector::parse("h2, h1.h2").unwrap();
    let title = document
        .select(&title_selector)
        .next()
        .map(|el| el.text().collect::<Vec<_>>().concat().trim().to_string())
        .unwrap_or_else(|| "Announcements".to_string());

    let intro_selector = Selector::parse("#intro .no-overflow").unwrap();
    let description = document
        .select(&intro_selector)
        .next()
        .map(|el| el.text().collect::<Vec<_>>().concat().trim().to_string())
        .unwrap_or_default();

    let row_selector = Selector::parse("tr.discussion").unwrap();
    let link_selector = Selector::parse("th.topic a, td.topic a").unwrap();
    let mut threads = Vec::new();

    for row in document.select(&row_selector) {
        let row_id = row.value().attr("data-discussionid").map(|s| s.to_string());

        let (id, title) = if let Some(link) = row.select(&link_selector).next() {
            let href = link.value().attr("href").unwrap_or("");
            let title = link.text().collect::<Vec<_>>().concat().trim().to_string();
            let parsed_id = if let Some(pos) = href.find("d=") {
                let id_str = &href[pos + 2..];
                let end_pos = id_str.find('&').unwrap_or(id_str.len());
                id_str[..end_pos].to_string()
            } else {
                row_id.unwrap_or_default()
            };
            (parsed_id, title)
        } else {
            (row_id.unwrap_or_default(), String::new())
        };

        if !id.is_empty() {
            threads.push(DiscussionThread { id, title });
        }
    }

    Ok(ForumDetails {
        id: forum_id,
        title,
        description,
        threads,
    })
}

// Custom parser to extract discussion details and posts
pub fn parse_discussion_html(html: &str, discuss_id: String) -> Result<DiscussionDetails, ErrorResponse> {
    let document = scraper::Html::parse_document(html);

    let title_selector = Selector::parse(".discussionname, h3.discussionname").unwrap();
    let title = document
        .select(&title_selector)
        .next()
        .map(|el| el.text().collect::<Vec<_>>().concat().trim().to_string())
        .unwrap_or_default();

    let article_selector = Selector::parse("article[data-region=\"post\"]").unwrap();
    let subject_selector = Selector::parse("[data-region-content=\"forum-post-core-subject\"], .post-heading").unwrap();
    let author_selector = Selector::parse(".author-time-info a, .post-footer a").unwrap();
    let time_selector = Selector::parse(".author-time-info time, .post-footer time").unwrap();

    let fullmessage_selector = Selector::parse(".fullmessage").unwrap();
    let content_container_selector = Selector::parse(".post-content-container").unwrap();

    let mut posts = Vec::new();

    for article in document.select(&article_selector) {
        let post_id = article
            .value()
            .attr("data-post-id")
            .map(|s| s.to_string())
            .unwrap_or_default();

        let subject = article
            .select(&subject_selector)
            .next()
            .map(|el| el.text().collect::<Vec<_>>().concat().trim().to_string())
            .unwrap_or_default();

        let author = article
            .select(&author_selector)
            .next()
            .map(|el| el.text().collect::<Vec<_>>().concat().trim().to_string())
            .unwrap_or_default();

        let time = article
            .select(&time_selector)
            .next()
            .map(|el| el.text().collect::<Vec<_>>().concat().trim().to_string())
            .or_else(|| {
                article
                    .select(&time_selector)
                    .next()
                    .and_then(|el| el.value().attr("datetime").map(|s| s.to_string()))
            })
            .unwrap_or_default();

        let content = if let Some(fm) = article.select(&fullmessage_selector).next() {
            fm.html().trim().to_string()
        } else if let Some(cc) = article.select(&content_container_selector).next() {
            cc.html().trim().to_string()
        } else {
            String::new()
        };

        posts.push(ForumPost {
            id: post_id,
            subject,
            author,
            time,
            content,
        });
    }

    Ok(DiscussionDetails {
        id: discuss_id,
        title,
        posts,
    })
}

async fn get_forum_html(moodle_session_cookie: String, forum_id: String) -> Result<String, ErrorResponse> {
    let jar: Arc<Jar> = Jar::default().into();
    jar.add_cookie_str(
        &moodle_session_cookie,
        &"https://ekursy.put.poznan.pl".parse::<Url>().unwrap(),
    );

    let client = Client::builder()
        .cookie_provider(jar)
        .default_headers(generic_firefox_headers())
        .build()
        .unwrap();

    let url = format!("https://ekursy.put.poznan.pl/mod/forum/view.php?id={}", forum_id);

    let Ok(resp) = client
        .get(&url)
        .header(HOST, HeaderValue::from_static("ekursy.put.poznan.pl"))
        .send()
        .await
    else {
        return Err(ErrorResponse::REMOTE_SERVER_DIDNT_RESPOND("ekursy forum".into()));
    };

    if resp.url().to_string().starts_with("https://ekursy.put.poznan.pl/login/index.php") {
        return Err(ErrorResponse::AUTH_FAILED("Session expired".into()));
    }

    let Ok(text) = resp.text().await else {
        return Err(ErrorResponse::REMOTE_SERVER_SENT_INVALID_DATA(
            "ekursy forum response couldnt be parsed".into(),
        ));
    };

    Ok(text)
}

async fn get_discussion_html(moodle_session_cookie: String, discuss_id: String) -> Result<String, ErrorResponse> {
    let jar: Arc<Jar> = Jar::default().into();
    jar.add_cookie_str(
        &moodle_session_cookie,
        &"https://ekursy.put.poznan.pl".parse::<Url>().unwrap(),
    );

    let client = Client::builder()
        .cookie_provider(jar)
        .default_headers(generic_firefox_headers())
        .build()
        .unwrap();

    let url = format!("https://ekursy.put.poznan.pl/mod/forum/discuss.php?d={}", discuss_id);

    let Ok(resp) = client
        .get(&url)
        .header(HOST, HeaderValue::from_static("ekursy.put.poznan.pl"))
        .send()
        .await
    else {
        return Err(ErrorResponse::REMOTE_SERVER_DIDNT_RESPOND("ekursy discussion".into()));
    };

    if resp.url().to_string().starts_with("https://ekursy.put.poznan.pl/login/index.php") {
        return Err(ErrorResponse::AUTH_FAILED("Session expired".into()));
    }

    let Ok(text) = resp.text().await else {
        return Err(ErrorResponse::REMOTE_SERVER_SENT_INVALID_DATA(
            "ekursy discussion response couldnt be parsed".into(),
        ));
    };

    Ok(text)
}

async fn api_forum_get_handler(headers: HeaderMap, Query(query): Query<ForumQuery>) -> Response {
    let Some(moodle_session_id) = headers.get(AUTHORIZATION) else {
        return ErrorResponse::AUTH_FAILED("Authorization header with moodle session missing".into())
            .into_json_response();
    };

    let moodle_session_cookie = String::from("MoodleSession=") + moodle_session_id.to_str().unwrap();

    let Some(forum_id) = query.id else {
        return ErrorResponse::BAD_REQUEST("id query param missing".into()).into_json_response();
    };

    match get_forum_html(moodle_session_cookie, forum_id.clone()).await {
        Ok(html) => match parse_forum_html(&html, forum_id) {
            Ok(details) => Json(json!({
                "msg": "OK",
                "forum": details
            }))
            .into_response(),
            Err(err) => err.into_json_response(),
        },
        Err(err) => err.into_json_response(),
    }
}

async fn api_discussion_get_handler(headers: HeaderMap, Query(query): Query<DiscussionQuery>) -> Response {
    let Some(moodle_session_id) = headers.get(AUTHORIZATION) else {
        return ErrorResponse::AUTH_FAILED("Authorization header with moodle session missing".into())
            .into_json_response();
    };

    let moodle_session_cookie = String::from("MoodleSession=") + moodle_session_id.to_str().unwrap();

    let Some(discuss_id) = query.id.or(query.d) else {
        return ErrorResponse::BAD_REQUEST("id or d query param missing".into()).into_json_response();
    };

    match get_discussion_html(moodle_session_cookie, discuss_id.clone()).await {
        Ok(html) => match parse_discussion_html(&html, discuss_id) {
            Ok(details) => Json(json!({
                "msg": "OK",
                "discussion": details
            }))
            .into_response(),
            Err(err) => err.into_json_response(),
        },
        Err(err) => err.into_json_response(),
    }
}
