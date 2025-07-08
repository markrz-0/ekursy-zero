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
