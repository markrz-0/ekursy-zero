use crate::{errors::ErrorResponse, parsers::{prepare_parser_response, Parser}};

use axum::response::Response;
use scraper::{selectable::Selectable, ElementRef, Selector};
use serde::{Serialize};


pub const NAME: &'static str = "merkury";

pub struct MerkuryParser;

#[allow(non_camel_case_types)]
#[derive(Debug, Serialize)]
enum ResourceKind {
    TITLE,
    DESCRIPTION,
    CARD_TITLE,
    CARD_DESCRIPTION,
    RESOURCE_DESCRIPTION,
    FILE,
    URL,
    QUIZ,
    OTHER,
    INVALID
}

#[derive(Debug, Serialize)]
struct Resource {
    kind: ResourceKind,
    data: Vec<String>,
    id: Option<String>
}

fn get_text<'a>(document: impl scraper::selectable::Selectable<'a>, selector: &Selector) -> Result<Vec<String>, ()>{
    let Some(element) = document.select(&selector).next()
        else { return Err(()) };

    let mut text = vec![];
    for text_node in element.text() {
        text.push(text_node.into());
    }

    Ok(text)
}

fn list_text_in_child_paragraphs<'a>(document: impl scraper::selectable::Selectable<'a>, selector: &Selector) -> Result<Vec<String>, ()> {
    let Some(element) = document.select(&selector).next()
        else { return Err(()) };
    
    let mut out = vec![];
    for p in element.select(&Selector::parse("p, li").unwrap()) {
        let mut text = String::new();
        for text_node in p.text(){
            text += text_node;
        }
        out.push(text);
    }

    Ok(out)
}

fn construct_resource<'a>(resource_element: ElementRef<'a>) -> Result<Resource, ErrorResponse> {
    let resource_kind;

    if resource_element.value().has_class("label", scraper::CaseSensitivity::AsciiCaseInsensitive) {
        resource_kind = ResourceKind::RESOURCE_DESCRIPTION;
    }
    else if resource_element.value().has_class("url", scraper::CaseSensitivity::AsciiCaseInsensitive) {
        resource_kind = ResourceKind::URL;
    }
    else if resource_element.value().has_class("quiz", scraper::CaseSensitivity::AsciiCaseInsensitive) {
        resource_kind = ResourceKind::QUIZ;
    }
    else if resource_element.value().has_class("resource", scraper::CaseSensitivity::AsciiCaseInsensitive) {
        resource_kind = ResourceKind::FILE;
    }
    else {
        resource_kind = ResourceKind::OTHER;
    }

    let text = match resource_kind {
        ResourceKind::RESOURCE_DESCRIPTION => list_text_in_child_paragraphs(resource_element, &Selector::parse(".contentwithoutlink").unwrap()),
        _ => get_text(resource_element, &Selector::parse(".activityinstance").unwrap())
    };

    let Ok(text_unwraped) = text
        else { return Err(ErrorResponse::REMOTE_SERVER_SENT_INVALID_DATA("invalid resource".into())) };

    let mut r = Resource {
        kind: resource_kind,
        data: text_unwraped,
        id: None,
    };

    if let Some(resource_link) = resource_element.select(&Selector::parse(".aalink").unwrap()).next() {
        if let Some(link) = resource_link.value().attr("href") {
            r.id = Some(link.replace("https://ekursy.put.poznan.pl/mod/", "").replace("/view.php?", "$"))
        }
    }

    Ok(r)
}

fn extract_resources<'a>(element: impl Selectable<'a>) -> Result<Vec<Resource>, ErrorResponse> {
    let mut out = vec![];
    let resource_selector = Selector::parse(".activity").unwrap();
    for resource in element.select(&resource_selector) {
        
        let r = match construct_resource(resource) {
            Ok(r) => r,
            Err(_) => Resource { kind: ResourceKind::INVALID, data: vec!["Invalid resource".into()], id: None }
        };

        out.push(r);
    }

    Ok(out)
}

fn try_parse(html_string: String) -> Result<Vec<Resource>, ErrorResponse> {
    let document = scraper::Html::parse_document(html_string.as_str());

    let mut out = vec![];

    // get title
    let Ok(h1_text) = get_text(&document, &Selector::parse("h1").unwrap())
        else { return Err(ErrorResponse::REMOTE_SERVER_SENT_INVALID_DATA("H1 not present".into())) };
    
    out.push(Resource { kind: ResourceKind::TITLE, data: h1_text, id: None });

    // description
    let Ok(desc) = list_text_in_child_paragraphs(&document, &Selector::parse(".summary:not(.card-text)").unwrap())
        else { return Err(ErrorResponse::REMOTE_SERVER_SENT_INVALID_DATA("desc not present".into())) };

    out.push(Resource { kind: ResourceKind::DESCRIPTION, data: desc, id: None });

    let cards_selector = Selector::parse(".card.section.main > div").unwrap();
    let cards = document.select(&cards_selector);
    for card in cards {
        let Ok(title) = get_text(card, &Selector::parse(".sectionname").unwrap())
            else { return Err(ErrorResponse::REMOTE_SERVER_SENT_INVALID_DATA("card title not present".into())) };
        out.push(Resource { kind: ResourceKind::CARD_TITLE, data: title, id: None });

        let Ok(desc) = get_text(card, &Selector::parse(".summary").unwrap())
            else { return Err(ErrorResponse::REMOTE_SERVER_SENT_INVALID_DATA("card desc not present".into())) };
        out.push(Resource { kind: ResourceKind::CARD_DESCRIPTION, data: desc, id: None });

        let resources = match extract_resources(card) {
            Ok(r) => r,
            Err(e) => return Err(e)
        };

        out.extend(resources);
    }

    let topics_selector = Selector::parse(".topics").unwrap();
    let mut topics = document.select(&topics_selector);
    let topics_container = topics.next();
    if topics_container.is_none() {
        return Ok(out)
    } 

    let sections_selector = Selector::parse(".section.main").unwrap();
    let sections = document.select(&sections_selector);
    for section in sections {
        let Ok(title) = get_text(section, &Selector::parse("h3").unwrap())
            else { return Err(ErrorResponse::REMOTE_SERVER_SENT_INVALID_DATA("title not present in section".into())) };
        out.push(Resource { kind: ResourceKind::CARD_TITLE, data: title, id: None });

        let resources = match extract_resources(section) {
            Ok(r) => r,
            Err(e) => return Err(e)
        };

        out.extend(resources);

    }


    Ok(out)
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