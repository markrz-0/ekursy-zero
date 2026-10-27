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
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

use crate::{errors::ErrorResponse, util::generic_firefox_headers, web::routes::course_list::get_sesskey};

// moodle caps limitnum for action events at 50
const PAGE_SIZE: usize = 50;

pub fn routes() -> Router {
    Router::new()
        .route("/api/course_deadlines", get(api_course_deadlines_get_handler))
}

#[derive(Deserialize)]
struct CourseDeadlinesQuery {
    id: Option<String>,
    from: Option<u64>,
    to: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct AjaxResponseAction {
    name: String,
    url: String,
    actionable: bool,
}

#[allow(unused)]
#[derive(Debug, Deserialize)]
struct AjaxResponseEvent {
    id: u64,
    name: String,
    activityname: Option<String>,
    modulename: Option<String>,
    instance: Option<u64>,
    eventtype: String,
    timestart: u64,
    timesort: u64,
    overdue: Option<bool>,
    url: String,
    action: Option<AjaxResponseAction>,
}

#[allow(unused)]
#[derive(Debug, Deserialize)]
struct AjaxResponseData {
    events: Vec<AjaxResponseEvent>,
    firstid: u64,
    lastid: u64,
}

#[derive(Debug, Deserialize)]
struct AjaxResponseException {
    message: String,
    errorcode: Option<String>,
}

#[derive(Debug, Deserialize)]
struct AjaxResponse {
    error: bool,
    data: Option<AjaxResponseData>,
    exception: Option<AjaxResponseException>,
}

#[derive(Debug, Serialize, Clone)]
struct Deadline {
    id: u64,
    name: String,
    activity_name: Option<String>,
    module: Option<String>,
    instance: Option<u64>,
    event_type: String,
    timestart: u64,
    timesort: u64,
    overdue: bool,
    url: String,
    action_name: Option<String>,
    action_url: Option<String>,
    actionable: bool,
}

impl From<AjaxResponseEvent> for Deadline {
    fn from(value: AjaxResponseEvent) -> Self {
        Deadline {
            id: value.id,
            name: value.name,
            activity_name: value.activityname,
            module: value.modulename,
            instance: value.instance,
            event_type: value.eventtype,
            timestart: value.timestart,
            timesort: value.timesort,
            overdue: value.overdue.unwrap_or(false),
            url: value.url,
            action_name: value.action.as_ref().map(|a| a.name.clone()),
            action_url: value.action.as_ref().map(|a| a.url.clone()),
            actionable: value.action.as_ref().map(|a| a.actionable).unwrap_or(false),
        }
    }
}

// moodle_session_cookie string in a format MoodleSession=XXXXX
// from/to are unix timestamps matched against event's timesort
async fn get_course_deadlines(moodle_session_cookie: String, course_id: u64, from: Option<u64>, to: Option<u64>) -> Result<Vec<Deadline>, ErrorResponse> {

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

    let sesskey = get_sesskey(text)?;

    let mut out: Vec<Deadline> = Vec::new();
    let mut after_event_id: u64 = 0;

    loop {
        let mut args = Map::new();
        args.insert("courseid".into(), json!(course_id));
        args.insert("aftereventid".into(), json!(after_event_id));
        args.insert("limitnum".into(), json!(PAGE_SIZE));
        if let Some(from) = from {
            args.insert("timesortfrom".into(), json!(from));
        }
        if let Some(to) = to {
            args.insert("timesortto".into(), json!(to));
        }

        let request_data = json!([
            {
                "args": Value::Object(args),
                "index": 0,
                "methodname": "core_calendar_get_action_events_by_course"
            }
        ]);

        let Ok(resp_data) = client
            .post("https://ekursy.put.poznan.pl/lib/ajax/service.php")
            .header(HOST, HeaderValue::from_static("ekursy.put.poznan.pl"))
            .json(&request_data)
            .query(&[
                ("info", "core_calendar_get_action_events_by_course"),
                ("sesskey", sesskey.as_str())
            ])
            .send()
            .await
            else { return Err(ErrorResponse::REMOTE_SERVER_DIDNT_RESPOND("ekursy ajax".into())) };

        let Ok(data_str) = resp_data.text().await
            else { return Err(ErrorResponse::REMOTE_SERVER_SENT_INVALID_DATA("ekursy ajax response couldnt be read".into())) };

        let Ok(mut data) = serde_json::from_str::<Vec<AjaxResponse>>(&data_str)
            else { return Err(ErrorResponse::REMOTE_SERVER_SENT_INVALID_DATA("ekursy ajax response couldnt be parsed".into())) };

        if data.is_empty() {
            return Err(ErrorResponse::REMOTE_SERVER_SENT_INVALID_DATA("ekursy ajax response was empty".into()));
        }
        let response = data.swap_remove(0);

        if response.error {
            let exception = response.exception;
            let errorcode = exception.as_ref().and_then(|e| e.errorcode.clone()).unwrap_or_default();
            let message = exception.map(|e| e.message).unwrap_or_else(|| "unknown error".into());
            return Err(match errorcode.as_str() {
                "servicerequireslogin" | "invalidsesskey" => ErrorResponse::AUTH_FAILED("Session expired".into()),
                "invalidrecord" | "requireloginerror" => ErrorResponse::BAD_REQUEST(format!("Cannot access course: {}", message)),
                _ => ErrorResponse::REMOTE_SERVER_SENT_INVALID_DATA(format!("ekursy ajax error: {}", message)),
            });
        }

        let Some(page) = response.data
            else { return Err(ErrorResponse::REMOTE_SERVER_SENT_INVALID_DATA("ekursy ajax response had no data".into())) };

        let page_len = page.events.len();
        out.extend(page.events.into_iter().map(Deadline::from));

        if page_len < PAGE_SIZE || page.lastid == 0 {
            break;
        }
        after_event_id = page.lastid;
    }

    out.sort_by_key(|d| d.timesort);

    Ok(out)
}

async fn api_course_deadlines_get_handler(headers: HeaderMap, Query(query): Query<CourseDeadlinesQuery>) -> Response {
    let Some(moodle_session_id) = headers.get(AUTHORIZATION)
        else { return ErrorResponse::AUTH_FAILED("Authorization header with moodle session missing".into()).into_json_response(); };

    let moodle_session_cookie = String::from("MoodleSession=") + moodle_session_id.to_str().unwrap();

    let Some(course_id) = query.id
        else { return ErrorResponse::BAD_REQUEST("id query param missing".into()).into_json_response(); };

    let Ok(course_id) = course_id.parse::<u64>()
        else { return ErrorResponse::BAD_REQUEST("id query param must be a number".into()).into_json_response(); };

    if let (Some(from), Some(to)) = (query.from, query.to) {
        if from > to {
            return ErrorResponse::BAD_REQUEST("from must not be greater than to".into()).into_json_response();
        }
    }

    let deadlines = match get_course_deadlines(moodle_session_cookie, course_id, query.from, query.to).await {
        Ok(d) => d,
        Err(r) => return r.into_json_response()
    };

    Json(json!({
        "msg": "OK",
        "deadlines": deadlines
    })).into_response()
}
