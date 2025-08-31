use std::sync::Arc;

use axum::{http::HeaderValue, response::{IntoResponse, Response}, routing::post, Json, Router};
use cookie::Cookie;
use reqwest::{cookie::{CookieStore, Jar}, header::{HOST, ORIGIN, REFERER}, Client, Url};
use scraper::Selector;
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::{errors::ErrorResponse, util::generic_firefox_headers};

pub fn routes() -> Router {
    Router::new()   
        .route("/api/login", post(api_login_post_handler))
}

#[derive(Debug, Deserialize)]
struct SignIn {
    email: String,
    password: String
}

#[derive(Debug, Serialize)]
struct EloginSignIn {
    login: String,
    password: String,
    csrf_token: String,
    send: String
}

// 2 means it's redirect number 2, not the word "to"
#[derive(Debug, Serialize, Default)]
#[allow(non_snake_case)]
struct Redirect2Data {
    version: String,
    language: String,
    gravatar_hash: String,
    gravatarHash: String, // it has to be in camelCase DO NOT RENAME
    sessionAuthorizationKey: String // it has to be in camelCase DO NOT RENAME
}

// returns (current page url [after redirects], login page text content)
async fn init_requests_chain(client: &Client) -> Result<(Url, String), ErrorResponse> {

    let Ok(resp) = client
        .get("https://elogin.put.poznan.pl/")
        .header(REFERER, HeaderValue::from_str("https://ekursy.put.poznan.pl/").unwrap())
        .header(HOST, HeaderValue::from_str("elogin.put.poznan.pl").unwrap())
        .query(&[("do", "Authorize"), ("system", "ekursy.esys.put.poznan.pl")])
        .send()
        .await
        else { return Err(ErrorResponse::REMOTE_SERVER_DIDNT_RESPOND("Init chain request failed".into())) };
    
    let url= resp.url().clone();

    let Ok(text) = resp.text().await
        else { return Err(ErrorResponse::UNABLE_TO_PARSE_RESPONSE_TEXT("Init chain request failed".into())) };

    Ok((url, text))
}

// token from hidden input
fn get_csrf_token(login_page_content: String) -> Result<String, ErrorResponse> {
    let document = scraper::Html::parse_document(login_page_content.as_str());

    let input_selector= Selector::parse("input[name=csrf_token]").unwrap();

    for element in document.select(&input_selector) {
        // there should only be 1 input with such selector -> we can return
        let Some(csrf_token) = element.value().attr("value")
            else { return Err(ErrorResponse::REMOTE_SERVER_SENT_INVALID_DATA("CSRF token field doesnt have value attr".into())) };

        return Ok(csrf_token.into());
    }

    Err(ErrorResponse::REMOTE_SERVER_SENT_INVALID_DATA("CSRF token field missing".into()))
}

// returns html of redirect page after login
async fn login_into_elogin(client: &Client, elogin_url: Url, elogin: &EloginSignIn) -> Result<String, ErrorResponse> {
    let Ok(resp) = client
        .post(elogin_url.clone())
        .header(REFERER, HeaderValue::from_str(elogin_url.clone().as_str()).unwrap())
        .header(HOST, HeaderValue::from_str("elogin.put.poznan.pl").unwrap())
        .header(ORIGIN, "https://elogin.put.poznan.pl")
        .form(elogin)
        .send()
        .await
        else { return Err(ErrorResponse::REMOTE_SERVER_DIDNT_RESPOND("Login request failed".into())) };


    if resp.url().as_str().starts_with(elogin_url.as_str()){
        return Err(ErrorResponse::AUTH_FAILED("Invalid login or password".into()))
    }

    let Ok(text) = resp.text().await
        else { return Err(ErrorResponse::UNABLE_TO_PARSE_RESPONSE_TEXT("Login request failed".into())) };

    Ok(text)
}

async fn send_redirect_forms(client: &Client, redirect2_data: &Redirect2Data) -> Result<(), ErrorResponse> {
    // browser does this request, it seams kinda useless
    let Ok(_) = client
        .post("https://ekursy.put.poznan.pl/auth/ekonto/redirect.php")
        .header(REFERER, HeaderValue::from_str("https://elogin.put.poznan.pl/").unwrap())
        .header(HOST, HeaderValue::from_str("ekursy.put.poznan.pl").unwrap())
        .header(ORIGIN, "https://elogin.put.poznan.pl")
        .form(redirect2_data)
        .send()
        .await
        else { return Err(ErrorResponse::REMOTE_SERVER_DIDNT_RESPOND("Redirect number 1 request failed".into())) };

    // this sets moodle session id
    let Ok(_) = client
        .post("https://ekursy.put.poznan.pl/login/index.php")
        .header(REFERER, HeaderValue::from_str("https://ekursy.put.poznan.pl/auth/ekonto/redirect.php").unwrap())
        .header(HOST, HeaderValue::from_str("ekursy.put.poznan.pl").unwrap())
        .header(ORIGIN, "https://elogin.put.poznan.pl")
        .form(redirect2_data)
        .send()
        .await
        else { return Err(ErrorResponse::REMOTE_SERVER_DIDNT_RESPOND("Redirect number 2 request failed".into())) };
    
    Ok(())
}

fn get_redirect2_data(redirect1_page_content: String) -> Result<Redirect2Data, ErrorResponse> {
    let document = scraper::Html::parse_document(redirect1_page_content.as_str());

    let selectors = vec![
        "input[name=version]",
        "input[name=language]",
        "input[name=gravatar_hash]",
        "input[name=gravatarHash]",
        "input[name=sessionAuthorizationKey]"
    ];

    let mut out = Redirect2Data::default();

    let mut idx = 0;
    for selector in selectors {
        let input_selector = Selector::parse(selector).unwrap();

        let Some(input) = document.select(&input_selector).next()
            else { return Err(ErrorResponse::REMOTE_SERVER_SENT_INVALID_DATA(format!("Missing input field: {}", selector))) };
            
        let Some(value) = input.value().attr("value")
            else { return Err(ErrorResponse::REMOTE_SERVER_SENT_INVALID_DATA(format!("Input field doesnt have value: {}", selector))) };

        // there surely is a better way... 
        match idx {
            0 => { out.version = value.into() }
            1 => { out.language = value.into() }
            2 => { out.gravatar_hash = value.into() }
            3 => { out.gravatarHash = value.into() }
            4 => { out.sessionAuthorizationKey = value.into() }
            _ => ()
        }

        idx += 1;
    }

    Ok(out)

}

async fn get_moodle_session_key(sign_in: SignIn) -> Result<String, ErrorResponse> {
    let jar: Arc<Jar> = Jar::default().into();

    let client = Client::builder()
        .default_headers(generic_firefox_headers())
        .cookie_provider(jar.clone())
        .build()
        .unwrap();

    let (elogin_url, login_page_content ) = match init_requests_chain(&client).await {
        Ok(v) => v, 
        Err(r) => return Err(r)
    };

    let csrf = match get_csrf_token(login_page_content) {
        Ok(v) => v, 
        Err(r) => return Err(r)
    };
    
    let elogin = EloginSignIn {
        login: sign_in.email,
        password: sign_in.password,
        csrf_token: csrf,
        send: "".into()
    };

    let redirect1_page_content = match login_into_elogin(&client, elogin_url, &elogin).await {
        Ok(v) => v, 
        Err(r) => return Err(r)
    };

    let redirect2_data = match get_redirect2_data(redirect1_page_content) {
        Ok(v) => v, 
        Err(r) => return Err(r)
    };

    match send_redirect_forms(&client, &redirect2_data).await {
        Ok(_) => (), 
        Err(r) => return Err(r)
    };

    if let Some(cookies) = jar.cookies(&"https://ekursy.put.poznan.pl/".parse::<Url>().unwrap()) {
        let cookies_string = cookies.to_str().unwrap();

        for cr in Cookie::split_parse(cookies_string) {
            if let Ok(c) = cr {
                if c.name() == "MoodleSession" {
                    return Ok(c.value().into())
                }
            }
        }

        return Ok(cookies_string.to_owned());
    }

    Err(ErrorResponse::REMOTE_SERVER_SENT_INVALID_DATA("Moodle Session not found in response".into()))
}

async fn api_login_post_handler(Json(login): Json<SignIn>) -> Response {
    let moodle_session_key = match get_moodle_session_key(login).await {
        Ok(k) => k,
        Err(r) => return r.into_json_response(),
    };

    Json(json!({
        "msg": "OK",
        "moodleSessionKey": moodle_session_key
    })).into_response()
}
