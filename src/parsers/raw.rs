use axum::response::Response;
use scraper::Selector;

use crate::{errors::ErrorResponse, parsers::{prepare_parser_response, Parser}};

pub const NAME: &'static str = "raw";
pub struct RawParser;

impl Parser for RawParser {
    fn parse(&self, html_string: String) -> Response {
        let document: scraper::Html = scraper::Html::parse_document(html_string.as_str());
        let Some(_) = document.select(&Selector::parse("div.course-content").unwrap()).next()
            else { return ErrorResponse::AUTH_FAILED("Session expired".into()).into_json_response() };

        prepare_parser_response(NAME.into(), html_string)
    }
}
