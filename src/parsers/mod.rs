use std::collections::HashMap;

use axum::{response::{IntoResponse, Response}, Json};
use serde::Serialize;
use serde_json::json;
use once_cell::sync::Lazy;

pub mod raw;
pub mod mercury;

pub trait Parser {
    fn parse(&self, html_string: String, course_id: String) -> Response;
}

pub fn prepare_parser_response(name: String, result: impl Serialize) -> Response {
    Json(json!({
        "msg": "OK",
        "parser": name,
        "result": result
    })).into_response()
}

static AVAILABLE_PARSERS: Lazy<HashMap<&'static str, Box<dyn Parser + Sync + Send>>> = Lazy::new(|| {
    let mut parsers: HashMap<&'static str, Box<dyn Parser + Sync + Send>> = HashMap::new();
    parsers.insert(raw::NAME, Box::new(raw::RawParser));
    parsers.insert(mercury::NAME, Box::new(mercury::MercuryParser));
    parsers
});


pub fn available_parsers() -> &'static HashMap<&'static str, Box<dyn Parser + Sync + Send>> {
    &AVAILABLE_PARSERS
}
