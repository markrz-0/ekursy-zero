#![allow(non_camel_case_types)]

use axum::{response::{Html, IntoResponse, Redirect, Response}, Json};
use reqwest::StatusCode;
use serde_json::json;

use super::to_safe_html;

pub struct ErrorResponseDetails {
    status_code: StatusCode,
    error_code: String,
    msg: String
}
pub enum ErrorResponse {
    TEMPLATE_FILE_ERROR(String),
    REMOTE_SERVER_DIDNT_RESPOND(String),
    UNABLE_TO_PARSE_RESPONSE_TEXT(String),
    REMOTE_SERVER_SENT_INVALID_DATA(String),
    AUTH_FAILED(String),
    BAD_REQUEST(String)
}

pub trait IntoJsonResponse {
    fn into_json_response(self) -> Response;
}

impl From<ErrorResponse> for ErrorResponseDetails {
    fn from(value: ErrorResponse) -> Self {
        match value {
            ErrorResponse::TEMPLATE_FILE_ERROR(msg) => 
                ErrorResponseDetails { 
                    status_code: StatusCode::NOT_FOUND,
                    error_code: "ERR-000".into(),
                    msg
                },
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

impl IntoJsonResponse for ErrorResponse {
    fn into_json_response(self) -> Response {
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

