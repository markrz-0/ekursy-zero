use std::sync::Arc;

use axum::{extract::Query, http::{HeaderMap, HeaderValue}, response::{IntoResponse, Redirect, Response}, routing::get, Router};
use reqwest::{cookie::Jar, header::{AUTHORIZATION, CONTENT_DISPOSITION, CONTENT_TYPE, HOST}, Client, Url};
use scraper::Selector;
use serde::Deserialize;

use crate::{errors::ErrorResponse, parsers::{available_parsers, mercury}, util::generic_firefox_headers};

pub fn routes() -> Router {
    Router::new()
        .route("/api/resource", get(resource_handler))
}


#[derive(Debug, Deserialize)]
struct ResourceQuery {
    id: Option<String>,
    kind: Option<String>
}

#[derive(Debug, Default)]
struct ResourceExtractorData {
    iframe_src: Option<String>,
    resource_url: Option<String>
}

fn is_mime_token(value: &str) -> bool {
    !value.is_empty()
        && value.bytes().all(|byte| {
            byte.is_ascii_alphanumeric()
                || matches!(
                    byte,
                    b'!' | b'#'
                        | b'$'
                        | b'%'
                        | b'&'
                        | b'\''
                        | b'*'
                        | b'+'
                        | b'-'
                        | b'.'
                        | b'^'
                        | b'_'
                        | b'`'
                        | b'|'
                        | b'~'
                )
        })
}

fn is_valid_content_type(value: &HeaderValue) -> bool {
    let Ok(value) = value.to_str() else {
        return false;
    };

    let media_type = value.split(';').next().unwrap_or_default().trim();
    let Some((type_name, subtype)) = media_type.split_once('/') else {
        return false;
    };

    is_mime_token(type_name.trim()) && is_mime_token(subtype.trim())
}

fn disposition_filename(value: &HeaderValue) -> Option<String> {
    let value = value.to_str().ok()?;

    value.split(';').skip(1).find_map(|parameter| {
        let (name, raw_value) = parameter.trim().split_once('=')?;
        if !name.eq_ignore_ascii_case("filename") && !name.eq_ignore_ascii_case("filename*") {
            return None;
        }

        let mut filename = raw_value.trim().trim_matches('"');
        if name.eq_ignore_ascii_case("filename*") {
            filename = filename
                .rsplit_once("''")
                .map_or(filename, |(_, encoded)| encoded);
        }

        (!filename.is_empty()).then(|| filename.to_owned())
    })
}

fn content_type_from_filename(filename: &str) -> Option<&'static str> {
    let extension = filename.rsplit_once('.')?.1.to_ascii_lowercase();

    match extension.as_str() {
        "ipynb" => Some("application/x-ipynb+json"),
        "pdf" => Some("application/pdf"),
        "json" => Some("application/json"),
        "txt" => Some("text/plain"),
        "csv" => Some("text/csv"),
        "htm" | "html" => Some("text/html"),
        "png" => Some("image/png"),
        "jpg" | "jpeg" => Some("image/jpeg"),
        "gif" => Some("image/gif"),
        "svg" => Some("image/svg+xml"),
        "zip" => Some("application/zip"),
        _ => None,
    }
}

fn normalized_resource_headers(upstream: &HeaderMap) -> HeaderMap {
    let mut headers = HeaderMap::new();

    let content_type = upstream
        .get(CONTENT_TYPE)
        .filter(|value| is_valid_content_type(value))
        .cloned()
        .or_else(|| {
            upstream
                .get(CONTENT_DISPOSITION)
                .and_then(disposition_filename)
                .and_then(|filename| content_type_from_filename(&filename))
                .map(HeaderValue::from_static)
        })
        .unwrap_or_else(|| HeaderValue::from_static("application/octet-stream"));

    headers.insert(CONTENT_TYPE, content_type);
    if let Some(content_disposition) = upstream.get(CONTENT_DISPOSITION) {
        headers.insert(CONTENT_DISPOSITION, content_disposition.clone());
    }

    headers
}

async fn resource_download_response(resp: reqwest::Response) -> Result<Response, ErrorResponse> {
    let headers = normalized_resource_headers(resp.headers());
    let Ok(bytes) = resp.bytes().await else {
        return Err(ErrorResponse::UNABLE_TO_PARSE_RESPONSE_TEXT(
            "Invalid bytes. WTF?".into(),
        ));
    };

    Ok((headers, bytes).into_response())
}

async fn extract_resource(req_url: &str, resp: reqwest::Response, client: &Client) -> Result<Response, ErrorResponse> {
    // external file redirect
    if req_url != resp.url().to_string() {
        let headers = resp.headers().clone();

        let Ok(bytes) = resp.bytes().await
            else { return Err(ErrorResponse::UNABLE_TO_PARSE_RESPONSE_TEXT("Bytes not recieved from remote WTF?".into())) };

        return Ok((headers.clone(), bytes).into_response())
    }

    let Ok(text) = resp.text().await
        else { return Err(ErrorResponse::REMOTE_SERVER_SENT_INVALID_DATA("ekursy course response couldnt be parsed".into())) };

    // weird scopy thing
    // https://github.com/rust-lang/rust/issues/63768
    // https://users.rust-lang.org/t/future-is-not-send-as-this-value-is-used-across-an-await-but-i-drop-the-value-before-the-await/57574/5
    let data: ResourceExtractorData = { 
        let document = scraper::Html::parse_document(text.as_str());
        let mut out: ResourceExtractorData = Default::default();
        
        let iframe = document.select(&Selector::parse("iframe").unwrap()).next();
        if iframe.is_some() {
            let src = iframe.unwrap().value().attr("src");
            if src.is_some() {
                out.iframe_src = Some(src.unwrap().to_owned())
            }
        }

        let resource_workaround_anchor = document.select(&Selector::parse(".resourceworkaround > a").unwrap()).next();
        if resource_workaround_anchor.is_some() {
            if let Some(url) = resource_workaround_anchor.unwrap().value().attr("href") {
                out.resource_url = Some(url.into());
            }
        }
        // non-Send variables document and iframe are implicitly dropped and dont infect the function
        // dropping them explicitly doesnt work (as in linked issue)
        out
    };

    if data.iframe_src.is_some() {
        let Ok(resp2) = client
            .get(&data.iframe_src.unwrap())
            .header(HOST, HeaderValue::from_str(crate::util::ekursy_host()).unwrap())
            .send()
            .await
            else { return Err(ErrorResponse::REMOTE_SERVER_DIDNT_RESPOND("iframe resource".into())) };
        
        return resource_download_response(resp2).await
    }

    if data.resource_url.is_some() {
        let Ok(resp2) = client
            .get(&data.resource_url.unwrap())
            .header(HOST, HeaderValue::from_str(crate::util::ekursy_host()).unwrap())
            .send()
            .await
            else { return Err(ErrorResponse::REMOTE_SERVER_DIDNT_RESPOND("weird redirect (new window) resource".into())) };
        
        return resource_download_response(resp2).await
    }

    // TODO: video test & other

    return Err(ErrorResponse::PARSER_FOR_THIS_RESOURCE_DOESNT_EXIST(req_url.into()));
}

async fn extract_url(resp: reqwest::Response) -> Result<Response, ErrorResponse> {
    
    let Ok(text) = resp.text().await
        else { return Err(ErrorResponse::REMOTE_SERVER_SENT_INVALID_DATA("ekursy course response couldnt be parsed".into())) };

    let document = scraper::Html::parse_document(text.as_str());
    
    let Some(urlworkaround_element) = document.select(&Selector::parse(".urlworkaround").unwrap()).next()
        else { return Err(ErrorResponse::REMOTE_SERVER_SENT_INVALID_DATA("Unable to select url wrapper in resource".into())) };

    let Some(link_element) = urlworkaround_element.select(&Selector::parse("a").unwrap()).next()
        else { return Err(ErrorResponse::REMOTE_SERVER_SENT_INVALID_DATA("Unable to select url element in resource".into())) };

    let Some(url) = link_element.value().attr("href")
        else { return Err(ErrorResponse::REMOTE_SERVER_SENT_INVALID_DATA("Unable to select url in resource".into())) };

    return Ok(Redirect::to(url).into_response());
}

async fn extract_assignment(resp: reqwest::Response) -> Result<Response, ErrorResponse> {
    let Ok(text) = resp.text().await
        else { return Err(ErrorResponse::REMOTE_SERVER_SENT_INVALID_DATA("ekursy course response couldnt be parsed".into())) };

    Ok(available_parsers()[mercury::NAME].as_ref().parse( text, "###".into()))
}

async fn extract_page(resp: reqwest::Response) -> Result<Response, ErrorResponse> {
    let Ok(text) = resp.text().await
        else { return Err(ErrorResponse::REMOTE_SERVER_SENT_INVALID_DATA("ekursy course response couldnt be parsed".into())) };

    Ok(available_parsers()[mercury::NAME].as_ref().parse( text, "###".into()))
}

async fn extract_folder(resp: reqwest::Response) -> Result<Response, ErrorResponse> {
    let Ok(text) = resp.text().await
        else { return Err(ErrorResponse::REMOTE_SERVER_SENT_INVALID_DATA("ekursy course response couldnt be parsed".into())) };

    Ok(available_parsers()[mercury::NAME].as_ref().parse( text, "###".into()))
}

// moodle_session_cookie string in a format MoodleSession=XXXXX
async fn fetch_resource(moodle_session_cookie: String, resource_id: String, resource_kind: String) -> Result<Response, ErrorResponse> {
    let jar: Arc<Jar> = Jar::default().into();

    jar.add_cookie_str(&moodle_session_cookie, &crate::util::ekursy_base_url().parse::<Url>().unwrap());

    let client = Client::builder()
        .cookie_provider(jar)
        .default_headers(generic_firefox_headers())
        .build()
        .unwrap();

    let url = format!("{}/mod/{}/view.php?id={}", crate::util::ekursy_base_url(), resource_kind, resource_id);

    let Ok(resp) = client
        .get(&url)
        .header(HOST, HeaderValue::from_str(crate::util::ekursy_host()).unwrap())
        .send()
        .await
        else { return Err(ErrorResponse::REMOTE_SERVER_DIDNT_RESPOND("ekursy resource".into())) };
    
    println!("PRE SESSION");
    if resp.url().to_string().starts_with(&format!("{}/login/index.php", crate::util::ekursy_base_url())) {
        return Err(ErrorResponse::AUTH_FAILED("Session expired".into()))
    }
    println!("SESSION IS GOOD");

    match resource_kind.as_str() {
        "resource" => return extract_resource(&url, resp, &client).await,
        "url" => return extract_url(resp).await,
        "assign" => return extract_assignment(resp).await,
        "page" => return extract_page(resp).await,
        "folder" => return extract_folder(resp).await,
        _ => return Err(ErrorResponse::PARSER_FOR_THIS_RESOURCE_DOESNT_EXIST(url.into()))
    } 
}

async fn resource_handler(headers: HeaderMap, Query(query): Query<ResourceQuery>) -> Response {
    let Some(moodle_session_id) = headers.get(AUTHORIZATION)
        else { return ErrorResponse::AUTH_FAILED("Authorization header with moodle session missing".into()).into_json_response(); };

    let moodle_session_cookie = String::from("MoodleSession=") + moodle_session_id.to_str().unwrap();

    let Some(resource_id) = query.id
        else { return ErrorResponse::BAD_REQUEST("id query param missing".into()).into_json_response(); };

    let Some(resource_kind) = query.kind
        else { return ErrorResponse::BAD_REQUEST("kind query param missing".into()).into_json_response(); };

    match fetch_resource(moodle_session_cookie, resource_id, resource_kind).await {
        Ok(r) => return r,
        Err(e ) => return e.into_json_response()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::StatusCode;
    use wiremock::{
        matchers::{method, path},
        Mock, MockServer, ResponseTemplate,
    };

    enum ExtractionPath {
        Iframe,
        ResourceWorkaround,
    }

    async fn extract_mocked_resource(
        extraction_path: ExtractionPath,
        download_response: ResponseTemplate,
    ) -> Response {
        let server = MockServer::start().await;
        let download_url = format!("{}/download", server.uri());
        let page_body = match extraction_path {
            ExtractionPath::Iframe => format!(r#"<iframe src="{download_url}"></iframe>"#),
            ExtractionPath::ResourceWorkaround => format!(
                r#"<div class="resourceworkaround"><a href="{download_url}">download</a></div>"#
            ),
        };

        Mock::given(method("GET"))
            .and(path("/page"))
            .respond_with(ResponseTemplate::new(200).set_body_string(page_body))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/download"))
            .respond_with(download_response)
            .mount(&server)
            .await;

        let client = Client::new();
        let page_url = format!("{}/page", server.uri());
        let page_response = client.get(&page_url).send().await.unwrap();

        extract_resource(&page_url, page_response, &client)
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn iframe_infers_notebook_type_and_preserves_filename() {
        let response = extract_mocked_resource(
            ExtractionPath::Iframe,
            ResponseTemplate::new(200)
                .insert_header("Content-Type", "application")
                .insert_header("Content-Disposition", r#"inline; filename="Lab3.ipynb""#)
                .set_body_bytes(br#"{"cells": []}"#.to_vec()),
        )
        .await;

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers().get(CONTENT_TYPE).unwrap(),
            "application/x-ipynb+json"
        );
        assert_eq!(
            response.headers().get(CONTENT_DISPOSITION).unwrap(),
            r#"inline; filename="Lab3.ipynb""#
        );
    }

    #[tokio::test]
    async fn resource_workaround_infers_type_when_content_type_is_missing() {
        let response = extract_mocked_resource(
            ExtractionPath::ResourceWorkaround,
            ResponseTemplate::new(200)
                .insert_header(
                    "Content-Disposition",
                    r#"attachment; filename="Lab2Students.ipynb""#,
                )
                .set_body_bytes(br#"{"cells": []}"#.to_vec()),
        )
        .await;

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers().get(CONTENT_TYPE).unwrap(),
            "application/x-ipynb+json"
        );
        assert_eq!(
            response.headers().get(CONTENT_DISPOSITION).unwrap(),
            r#"attachment; filename="Lab2Students.ipynb""#
        );
    }

    #[tokio::test]
    async fn malformed_type_without_usable_filename_falls_back_to_octet_stream() {
        let response = extract_mocked_resource(
            ExtractionPath::Iframe,
            ResponseTemplate::new(200)
                .insert_header("Content-Type", "application")
                .insert_header("Content-Disposition", "inline")
                .set_body_bytes(vec![1, 2, 3]),
        )
        .await;

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers().get(CONTENT_TYPE).unwrap(),
            "application/octet-stream"
        );
        assert_eq!(
            response.headers().get(CONTENT_DISPOSITION).unwrap(),
            "inline"
        );
    }

    #[tokio::test]
    async fn valid_content_type_is_preserved() {
        let response = extract_mocked_resource(
            ExtractionPath::ResourceWorkaround,
            ResponseTemplate::new(200)
                .insert_header("Content-Type", "application/pdf")
                .insert_header("Content-Disposition", r#"inline; filename="notes.pdf""#)
                .set_body_bytes(b"%PDF".to_vec()),
        )
        .await;

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers().get(CONTENT_TYPE).unwrap(),
            "application/pdf"
        );
        assert_eq!(
            response.headers().get(CONTENT_DISPOSITION).unwrap(),
            r#"inline; filename="notes.pdf""#
        );
    }
}
