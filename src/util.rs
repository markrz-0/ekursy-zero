use axum::http::{HeaderMap, HeaderValue};
use reqwest::header::{ACCEPT, ACCEPT_ENCODING, ACCEPT_LANGUAGE, CONNECTION, UPGRADE_INSECURE_REQUESTS, USER_AGENT};

// "generic" headers mean:
// User-Agent, Accept, Accept-Language, Accept-Encoding, Connection, Upgrade-Insecure-Requests,  Sec-Fetch-Dest,  Sec-Fetch-Mode, Sec-Fetch-Site, Sec-Fetch-User, Priority
// not implemented page specific important headers: Host, Referer, Origin
pub fn generic_firefox_headers() -> HeaderMap<HeaderValue> {
    let mut header_map = HeaderMap::new();

    header_map.append(USER_AGENT, HeaderValue::from_static("Mozilla/5.0 (Windows NT 10.0; Win64; x64; rv:132.0) Gecko/20100101 Firefox/132.0"));
    header_map.append(ACCEPT, HeaderValue::from_static("text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8"));
    header_map.append(ACCEPT_LANGUAGE, HeaderValue::from_static("en-US,en;q=0.5"));
    header_map.append(ACCEPT_ENCODING, HeaderValue::from_static("gzip, deflate, br, zstd"));
    header_map.append(CONNECTION, HeaderValue::from_static("keep-alive"));
    header_map.append(UPGRADE_INSECURE_REQUESTS, HeaderValue::from_static("1"));
    header_map.append("Sec-Fetch-Dest", HeaderValue::from_static("document"));
    header_map.append("Sec-Fetch-Mode", HeaderValue::from_static("navigate"));
    header_map.append("Sec-Fetch-Site", HeaderValue::from_static("same-site"));
    header_map.append("Sec-Fetch-User", HeaderValue::from_static("?1"));
    header_map.append("Priority", HeaderValue::from_static("u=0, i"));

    return header_map;
}

use std::sync::OnceLock;

static EKURSY_BASE_URL: OnceLock<String> = OnceLock::new();
static ELOGIN_BASE_URL: OnceLock<String> = OnceLock::new();

pub fn ekursy_base_url() -> &'static str {
    EKURSY_BASE_URL.get_or_init(|| {
        std::env::var("EKURSY_BASE_URL").unwrap_or_else(|_| "https://ekursy.put.poznan.pl".to_string())
    })
}

pub fn elogin_base_url() -> &'static str {
    ELOGIN_BASE_URL.get_or_init(|| {
        std::env::var("ELOGIN_BASE_URL").unwrap_or_else(|_| "https://elogin.put.poznan.pl".to_string())
    })
}

pub fn ekursy_host() -> &'static str {
    static EKURSY_HOST: OnceLock<String> = OnceLock::new();
    EKURSY_HOST.get_or_init(|| {
        let url = reqwest::Url::parse(ekursy_base_url()).unwrap();
        url.host_str().unwrap_or("ekursy.put.poznan.pl").to_string()
    })
}

pub fn elogin_host() -> &'static str {
    static ELOGIN_HOST: OnceLock<String> = OnceLock::new();
    ELOGIN_HOST.get_or_init(|| {
        let url = reqwest::Url::parse(elogin_base_url()).unwrap();
        url.host_str().unwrap_or("elogin.put.poznan.pl").to_string()
    })
}

