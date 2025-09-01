use std::sync::Arc;

use axum::{http::{HeaderMap, HeaderValue}, response::{IntoResponse, Response}, routing::get, Json, Router};
use reqwest::{cookie::Jar, header::{AUTHORIZATION, HOST}, Client, Url};
use scraper::Selector;
use serde::{Deserialize, Serialize};
use serde_json::json;
use crate::{errors::ErrorResponse, util::generic_firefox_headers};

pub fn routes() -> Router {
    Router::new()
        .route("/api/course_list", get(api_course_list_get_handler))
}

#[allow(unused)]
#[derive(Debug, Deserialize)]
struct AjaxResponseCourse {
    id: u32,
    fullname: String,
    shortname: String,
    idnumber: String,
    summary: String,
    summaryformat: u8,
    startdate: u64,
    enddate: u64,
    visible: bool,
    showactivitydates: bool,
    showcompletionconditions: Option<bool>,
    fullnamedisplay: String,
    viewurl: String,
    courseimage: String,
    progress: u8,
    hasprogress: bool,
    isfavourite: bool,
    hidden: bool,
    showshortname: bool,
    coursecategory: String,
}

#[allow(unused)]
#[derive(Debug, Deserialize)]
struct AjaxResponseData {
    courses: Vec<AjaxResponseCourse>,
    nextoffset: i32
}


#[allow(unused)]
#[derive(Debug, Deserialize)]
struct AjaxResponse {
    error: bool,
    data: AjaxResponseData
}

#[derive(Debug, Serialize, Clone)]
struct Course {
    name: String,
    id: String,
    category: String
}

#[derive(Debug)]
struct ParsingError;

impl TryFrom<&AjaxResponseCourse> for Course {

    type Error = ParsingError;

    fn try_from(value: &AjaxResponseCourse) -> Result<Self, Self::Error> {
        let mut viewurl = value.viewurl.clone();
        let Some(idx) = viewurl.find("id=")
            else { return Err(ParsingError) };
        let id = viewurl.split_off(idx + 3);
        Ok(Course { 
            name: value.fullname.clone(), 
            id: id,
            category: value.coursecategory.clone()
        })
    }
}

fn get_sesskey(html: String) -> Result<String, ErrorResponse> {
    let document = scraper::Html::parse_document(html.as_str());
    let selector = Selector::parse("input[name=sesskey]").unwrap();
    let Some(element) = document.select(&selector).next()
        else { return Err(ErrorResponse::AUTH_FAILED("Session expired".into())) }; // technically it is invalid data but such response is mainly expected if session expired

    let Some(sesskey) = element.attr("value")
        else { return Err(ErrorResponse::REMOTE_SERVER_SENT_INVALID_DATA("Server didn't sent session key".into())) };

    return Ok(String::from(sesskey));
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

    let Ok(resp_dashboard) = client
        .get("https://ekursy.put.poznan.pl/my")
        .header(HOST, HeaderValue::from_static("ekursy.put.poznan.pl"))
        .send()
        .await
        else { return Err(ErrorResponse::REMOTE_SERVER_DIDNT_RESPOND("ekursy dashboard".into())) };

    let Ok(text) = resp_dashboard.text().await
        else { return Err(ErrorResponse::REMOTE_SERVER_SENT_INVALID_DATA("ekursy dashboard response couldnt be parsed".into())) };


    // get sesskey from response
    let sesskey = get_sesskey(text)?;

    let filter_data = json!([
        {
            "args": {
                "offset": 0,
                "limit": 0,
                "classification": "all",
                "customfieldname": "",
                "customfieldvalue": ""
            },
            "index": 0,
            "methodname": "core_course_get_enrolled_courses_by_timeline_classification"
        }
    ]);

    let Ok(resp_data) = client
        .post("https://ekursy.put.poznan.pl/lib/ajax/service.php")
        .header(HOST, HeaderValue::from_static("ekursy.put.poznan.pl"))
        .json(&filter_data)
        .query(&[
            ("info", "core_course_get_enrolled_courses_by_timeline_classification"),
            ("sesskey", sesskey.as_str())
        ])
        .send()
        .await
        else { return Err(ErrorResponse::REMOTE_SERVER_DIDNT_RESPOND("ekursy ajax".into())) };

    let Ok(data_str) = resp_data.text().await
        else { return Err(ErrorResponse::REMOTE_SERVER_SENT_INVALID_DATA("ekursy ajax response couldnt be read".into())) };

    let Ok(data) = serde_json::from_str::<Vec<AjaxResponse>>(&data_str)
        else { return Err(ErrorResponse::REMOTE_SERVER_SENT_INVALID_DATA("ekursy ajax response couldnt be parsed".into())) };

    let parsed_courses: Vec<Result<Course, ParsingError>> = data[0].data.courses.iter()
        .map(|c| Course::try_from(c))
        .collect();

    if parsed_courses.iter().any(|r| r.is_err()) {
        return Err(ErrorResponse::UNABLE_TO_PARSE_RESPONSE_TEXT("Couldnt parse course's visiturl for id".into()))
    }

    let mut out: Vec<Course> = parsed_courses.iter().map(|c| c.as_ref().unwrap()).cloned().collect();

    out.sort_by(|a , b| a.name.cmp(&b.name));

    Ok(out)
}

async fn api_course_list_get_handler(headers: HeaderMap) -> Response {
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