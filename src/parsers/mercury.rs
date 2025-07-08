use crate::{errors::ErrorResponse, parsers::{prepare_parser_response, Parser}};

use axum::response::Response;
use scraper::{selectable::Selectable, ElementRef, Selector};
use serde::{Serialize};


pub const NAME: &'static str = "mercury";

pub struct MercuryParser;


#[derive(Debug, Serialize)]
#[serde(tag = "type", rename_all = "lowercase")]
enum PageFragment {
    Text{ text: String },
    Link{ text: String, link: String },
    Iframe { src: String },
    Resource{ text: String, id: String, kind: String }
}

fn parse_a_tag(a_tag: ElementRef) -> Option<PageFragment> {
    let href = a_tag.attr("href").unwrap_or("#");
    if href.starts_with("#") {
        match get_trimmed_text(a_tag) {
            Some(text) => return Some(PageFragment::Text{ text: text }),
            None => return None
        }
    }

    if ! href.starts_with("https://ekursy.put.poznan.pl/mod/") {
        return Some(PageFragment::Link{ text: get_trimmed_text(a_tag).unwrap_or_default(), link: href.into() })
    }

    let resource_suffix = href.replace("https://ekursy.put.poznan.pl/mod/", "");
    let resource_data = resource_suffix.split("/view.php?id=").collect::<Vec<_>>();

    if resource_data.len() != 2 {
        return Some(PageFragment::Link{ text: get_trimmed_text(a_tag).unwrap_or_default(), link: href.into() })
    }

    Some(PageFragment::Resource {
        text: get_trimmed_text(a_tag).unwrap_or_default(),
        id: resource_data[1].into(),
        kind: resource_data[0].into()
    })
}

fn try_explore_children(root: ElementRef)  -> Result<Vec<PageFragment>, ErrorResponse> {
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
                out.push(PageFragment::Text { text: trimmed.into() });
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

fn try_parse_html_tree(root: ElementRef) -> Result<Vec<PageFragment>, ErrorResponse> {
    let mut out = vec![];

    if root.value().has_class("accesshide", scraper::CaseSensitivity::AsciiCaseInsensitive) {
        return Ok(out);
    }

    match root.value().name().to_lowercase().as_str() {
        "style" => (), // WHY TF WAS STYLE INSIDE A BODY TREE WTF
        "p" => { 
            match get_trimmed_text(root) {
                Some(text) => out.push(PageFragment::Text{ text: text }),
                None => ()
            }
        },
        "a" => {
            match parse_a_tag(root) {
                Some(fragment) => out.push(fragment),
                None => ()
            }
        },
        "iframe" => {
            if let Some(src) = root.attr("src") {
                out.push(PageFragment::Iframe { src: src.into() });
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
                    Some(text) => out.push(PageFragment::Text{ text: text }),
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

fn try_parse(html_string: String) -> Result<Vec<PageFragment>, ErrorResponse> {
    let document = scraper::Html::parse_document(html_string.as_str());

    let Some(content) = document.select(&Selector::parse("div.course-content").unwrap()).next()
        else { return Err(ErrorResponse::AUTH_FAILED("Session expired".into())) };

    try_parse_html_tree(content)
}

impl Parser for MercuryParser {
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