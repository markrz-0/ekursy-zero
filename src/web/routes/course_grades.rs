#![allow(non_camel_case_types)]

use std::sync::Arc;

use axum::{Json, Router, extract::Query, http::{HeaderMap, HeaderValue}, response::{IntoResponse, Response}, routing::get};
use reqwest::{cookie::Jar, header::{AUTHORIZATION, HOST}, Client, Url};
use scraper::{Html, Selector};
use serde::{Deserialize, Serialize};
use serde_json::json;


use crate::util::generic_firefox_headers;

use crate::errors::ErrorResponse;

pub fn routes() -> Router {
    Router::new()
        .route("/api/course_grades", get(api_course_get_handler))
}

#[derive(Deserialize)]
struct CourseQuery {
    id: Option<String>
}

/// Represents a node in the gradebook (either a category or an item)
/// Structs are derived with Serialize to easily return as JSON in axum.
#[derive(Debug, Serialize, Clone)]
pub struct GradeNode {
    // `skip` avoids outputting the internal hierarchy level to the JSON response
    #[serde(skip)]
    pub level: usize,
    pub is_category: bool,
    pub name: String,
    pub weight: String,
    pub grade: String,
    pub range: String,
    pub percentage: String,
    pub letter_grade: String,
    pub feedback: String,
    pub contribution: String,
    pub children: Vec<GradeNode>,
}

/// A temporary flat representation of an HTML row
struct ParsedRow {
    level: usize,
    is_category: bool,
    name: String,
    weight: String,
    grade: String,
    range: String,
    percentage: String,
    letter_grade: String,
    feedback: String,
    contribution: String,
}

// moodle_session_cookie string in a format MoodleSession=XXXXX
async fn get_course_grades_html(moodle_session_cookie: String, course_id: String) -> Result<String, ErrorResponse> {
    let jar: Arc<Jar> = Jar::default().into();

    jar.add_cookie_str(&moodle_session_cookie, &"https://ekursy.put.poznan.pl".parse::<Url>().unwrap());

    let client = Client::builder()
        .cookie_provider(jar)
        .default_headers(generic_firefox_headers())
        .build()
        .unwrap();

    let url = format!("https://ekursy.put.poznan.pl/grade/report/user/index.php?id={}", course_id);

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

pub fn parse_moodle_grades(html_content: &str) -> Vec<GradeNode> {
    let document = Html::parse_document(html_content);
    
    // Compile selectors once for performance
    let tr_selector = Selector::parse("table.user-grade tbody tr").unwrap();
    let th_selector = Selector::parse("th.column-itemname").unwrap();
    
    // Selects the span directly inside category-content (skipping the <a> tags with expand/collapse)
    let category_name_selector = Selector::parse("div.category-content > span").unwrap();
    let item_name_selector = Selector::parse(".gradeitemheader, .rowtitle").unwrap();

    let weight_sel = Selector::parse("td.column-weight").unwrap();
    let grade_sel = Selector::parse("td.column-grade").unwrap();
    let range_sel = Selector::parse("td.column-range").unwrap();
    let percent_sel = Selector::parse("td.column-percentage").unwrap();
    let letter_sel = Selector::parse("td.column-lettergrade").unwrap();
    let feedback_sel = Selector::parse("td.column-feedback").unwrap();
    let contrib_sel = Selector::parse("td.column-contributiontocoursetotal").unwrap();

    let mut flat_rows = Vec::new();

    for tr in document.select(&tr_selector) {
        // Skip visual spacer rows
        if tr.value().classes().any(|c| c == "spacer") {
            continue;
        }

        // Locate the header cell containing the name and level classes
        let th = match tr.select(&th_selector).next() {
            Some(th) => th,
            None => continue,
        };

        let classes: Vec<&str> = th.value().classes().collect();

        // Extract level number (e.g., "level1" -> 1)
        let level = classes
            .iter()
            .find(|c| c.starts_with("level"))
            .and_then(|s| s.trim_start_matches("level").parse::<usize>().ok())
            .unwrap_or(0);

        let is_category = classes.contains(&"category");

        // Extract Name based on row type
        let name = if is_category {
            th.select(&category_name_selector)
                .next()
                .map(|el| el.text().collect::<String>())
                .unwrap_or_default()
        } else {
            th.select(&item_name_selector)
                .next()
                .map(|el| el.text().collect::<String>())
                .unwrap_or_default()
        };

        // Helper to extract clean text from a column
        let get_col_text = |sel: &Selector| -> String {
            tr.select(sel)
                .next()
                .map(|el| {
                    el.text()
                        .collect::<String>()
                        .replace('\u{a0}', " ") // Remove Non-Breaking Spaces
                        .trim()
                        .to_string()
                })
                .unwrap_or_default()
        };

        flat_rows.push(ParsedRow {
            level,
            is_category,
            name: name.trim().to_string(),
            weight: get_col_text(&weight_sel),
            grade: get_col_text(&grade_sel),
            range: get_col_text(&range_sel),
            percentage: get_col_text(&percent_sel),
            letter_grade: get_col_text(&letter_sel),
            feedback: get_col_text(&feedback_sel),
            contribution: get_col_text(&contrib_sel),
        });
    }

    // Convert flat rows into a hierarchical tree using a Stack
    let mut roots = Vec::new();
    let mut stack: Vec<GradeNode> = Vec::new();

    for row in flat_rows {
        let node = GradeNode {
            level: row.level,
            is_category: row.is_category,
            name: row.name,
            weight: row.weight,
            grade: row.grade,
            range: row.range,
            percentage: row.percentage,
            letter_grade: row.letter_grade,
            feedback: row.feedback,
            contribution: row.contribution,
            children: Vec::new(),
        };

        // Pop elements off the stack if we've moved up the hierarchy
        while let Some(top) = stack.last() {
            if top.level >= node.level {
                let popped = stack.pop().unwrap();
                if let Some(parent) = stack.last_mut() {
                    parent.children.push(popped);
                } else {
                    roots.push(popped);
                }
            } else {
                break; // Found the correct parent
            }
        }
        
        // Push the new node onto the stack to act as a potential parent
        stack.push(node);
    }

    // Flush remaining items in the stack to their parents or the root
    while let Some(popped) = stack.pop() {
        if let Some(parent) = stack.last_mut() {
            parent.children.push(popped);
        } else {
            roots.push(popped);
        }
    }

    roots
}

async fn api_course_get_handler(headers: HeaderMap, Query(query): Query<CourseQuery>) -> Response {


    let Some(moodle_session_id) = headers.get(AUTHORIZATION)
        else { return ErrorResponse::AUTH_FAILED("Authorization header with moodle session missing".into()).into_json_response(); };

    let moodle_session_cookie = String::from("MoodleSession=") + moodle_session_id.to_str().unwrap();

    let Some(course_id) = query.id
        else { return ErrorResponse::BAD_REQUEST("id query param missing".into()).into_json_response(); };
    

    let html = match get_course_grades_html(moodle_session_cookie, course_id.clone()).await {
        Ok(h) => h,
        Err(r) => return r.into_json_response(),
    };

    Json(json!({
        "msg": "OK",
        "result": parse_moodle_grades(&html)
    })).into_response()
}
