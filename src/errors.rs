use axum::{response::{Html, IntoResponse, Redirect, Response}, Json};
use reqwest::StatusCode;
use serde_json::json;

use crate::util::to_safe_html;

pub struct ErrorResponseDetails {
    status_code: StatusCode,
    error_code: String,
    msg: String
}

#[allow(non_camel_case_types)]
pub enum ErrorResponse {
    REMOTE_SERVER_DIDNT_RESPOND(String),
    UNABLE_TO_PARSE_RESPONSE_TEXT(String),
    REMOTE_SERVER_SENT_INVALID_DATA(String),
    AUTH_FAILED(String),
    BAD_REQUEST(String)
}

impl From<ErrorResponse> for ErrorResponseDetails {
    fn from(value: ErrorResponse) -> Self {
        match value {
            ErrorResponse::REMOTE_SERVER_DIDNT_RESPOND(msg) => 
                ErrorResponseDetails { 
                    status_code: StatusCode::INTERNAL_SERVER_ERROR,
                    error_code: "ERR-001".into(),
                    msg
                },
            ErrorResponse::UNABLE_TO_PARSE_RESPONSE_TEXT(msg) => 
                ErrorResponseDetails { 
                    status_code: StatusCode::INTERNAL_SERVER_ERROR,
                    error_code: "ERR-002".into(),
                    msg
                },
            ErrorResponse::REMOTE_SERVER_SENT_INVALID_DATA(msg) =>
                ErrorResponseDetails { 
                    status_code: StatusCode::INTERNAL_SERVER_ERROR,
                    error_code: "ERR-003".into(),
                    msg
                },
            ErrorResponse::AUTH_FAILED(msg) =>
                ErrorResponseDetails { 
                    status_code: StatusCode::UNAUTHORIZED,
                    error_code: "ERR-004".into(),
                    msg
                },
            ErrorResponse::BAD_REQUEST(msg) =>
                ErrorResponseDetails { 
                    status_code: StatusCode::BAD_REQUEST,
                    error_code: "ERR-005".into(),
                    msg
                }
        }
    }
}

impl ErrorResponse {
    pub fn into_json_response(self) -> Response {
        let details = ErrorResponseDetails::from(self);

        (
            details.status_code,
            Json(json!({
                "error": details.error_code,
                "msg": details.msg
            }))
        ).into_response()
    }
}

impl IntoResponse for ErrorResponse {
    fn into_response(self) -> Response {
        let details = ErrorResponseDetails::from(self);

        if details.status_code == StatusCode::UNAUTHORIZED {
            return Redirect::to(format!("/?invalid={}", urlencoding::encode(details.msg.as_str())).as_str()).into_response()
        }

        let html = format!("
            <h1>Error</h1><br/>
            <a href=\"/\">go back to login</a><br/>
            <p>{}</p> <br /> 
            <p>{}</p>", 
            to_safe_html(&details.error_code),
            to_safe_html(&details.msg)
        );
        
        (details.status_code, Html(html)).into_response()
    }
}

