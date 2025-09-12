use axum::{response::{IntoResponse, Response}, Json};
use reqwest::StatusCode;
use serde_json::json;

pub struct ErrorResponseDetails {
    status_code: StatusCode,
    error_code: String,
    msg: String
}

#[allow(non_camel_case_types)]
pub enum ErrorResponse {
    REMOTE_SERVER_DIDNT_RESPOND(String),
    PARSER_FOR_THIS_RESOURCE_DOESNT_EXIST(String),
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
                },
            ErrorResponse::PARSER_FOR_THIS_RESOURCE_DOESNT_EXIST(msg) => 
                ErrorResponseDetails { 
                    status_code: StatusCode::UNPROCESSABLE_ENTITY,
                    error_code: "ERR-006".into(),
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
