use crate::{errors::ErrorResponse, parsers::{prepare_parser_response, Parser}};

use axum::response::Response;
use scraper::{selectable::Selectable, ElementRef, Selector};
use serde::{Serialize};


pub const NAME: &'static str = "merkury";

pub struct MerkuryParser;


#[derive(Debug, Serialize)]
#[serde(tag = "type", rename_all = "lowercase")]
enum Resource {
    Text{ text: String },
    Link{ text: String, link: String },
    Iframe { src: String }
}

fn try_explore_children(root: ElementRef)  -> Result<Vec<Resource>, ErrorResponse> {
    let mut out = vec![];

    if let Some(action) = root.attr("data-action") { // for this stupid "Mark as completed" button
        if action == "toggle-manual-completion" {
            return Ok(out);
        }
    }

    for child in root.children() {
        if let Some(elem) = ElementRef::wrap(child) {
            match try_parse_html_tree(elem) {
                Ok(res) => out.extend(res),
                Err(err) => return Err(err)
            }
        } else if child.value().is_text() {
            let text = child.value().as_text().unwrap().to_string();
            let trimmed = text.trim();
            if trimmed.len() > 0 {
                out.push(Resource::Text { text: trimmed.into() });
            }
        }
    }
    Ok(out)
}

// returns None if after trimming text == ""
fn get_trimmed_text(root: ElementRef) -> Option<String> {
    let text = root.text().collect::<Vec<_>>().concat();
    let trimmed = text.trim();
    if trimmed.len() == 0 {
        return None
    }
    return Some(trimmed.into())
}

fn try_parse_html_tree(root: ElementRef) -> Result<Vec<Resource>, ErrorResponse> {
    let mut out = vec![];
    match root.value().name().to_lowercase().as_str() {
        "style" => (), // WHY TF WAS STYLE INSIDE A BODY TREE WTF
        "p" => { 
            match get_trimmed_text(root) {
                Some(text) => out.push(Resource::Text{ text: text }),
                None => ()
            }
        },
        "a" => {
            let href = root.attr("href").unwrap_or("#");
            if href.starts_with("#") {
                match get_trimmed_text(root) {
                    Some(text) => out.push(Resource::Text{ text: text }),
                    None => ()
                }
            } else {
                out.push(Resource::Link{ text: get_trimmed_text(root).unwrap_or_default(), link: href.into() });
            }
        },
        "iframe" => {
            if let Some(src) = root.attr("src") {
                out.push(Resource::Iframe { src: src.into() });
            }
        },
        "span" | "li" => {
            if let Some(_) = root.select(&Selector::parse("div, p, a").unwrap()).next() {
                match try_explore_children(root) {
                    Ok(res) => out.extend(res),
                    Err(err) => return Err(err)
                }
            } else {
                match get_trimmed_text(root) {
                    Some(text) => out.push(Resource::Text{ text: text }),
                    None => ()
                }
            }
        },
        _ => {
            match try_explore_children(root) {
                Ok(res) => out.extend(res),
                Err(err) => return Err(err)
            }
        }
    }
    Ok(out)
}

fn try_parse(html_string: String) -> Result<Vec<Resource>, ErrorResponse> {
    let document = scraper::Html::parse_document(html_string.as_str());

    let Some(content) = document.select(&Selector::parse("div.course-content").unwrap()).next()
        else { return Err(ErrorResponse::AUTH_FAILED("Session expired".into())) };

    try_parse_html_tree(content)
}

impl Parser for MerkuryParser {
    fn parse(&self, html_string: String) -> Response {

        match try_parse(html_string) {
            Ok(result) => {
                return prepare_parser_response(NAME.into(), result)
            },
            Err(err) => {
                return err.into_json_response()
            }
        }
    }
}