use axum::{
    body::Body,
    http::{header::{AUTHORIZATION, CONTENT_TYPE}, Request, StatusCode},
    Router,
};
use serde_json::{json, Value};
use std::env;
use std::sync::Mutex;
use tower::ServiceExt; // for oneshot
use wiremock::{
    matchers::{method, path, query_param},
    Mock, MockServer, ResponseTemplate,
};

// Mutex to serialize environment-variable-changing tests
static ENV_MUTEX: Mutex<()> = Mutex::new(());

fn get_app() -> Router {
    Router::new()
        .merge(crate::web::routes())
        .layer(tower_cookies::CookieManagerLayer::new())
}

struct TestContext {
    _guard: std::sync::MutexGuard<'static, ()>,
    mock_server: Option<MockServer>,
    is_live: bool,
    email: String,
    password: String,
}

async fn setup_test_context() -> TestContext {
    let guard = ENV_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
    
    // Load .env file
    dotenvy::dotenv().ok();

    let email = env::var("MOODLE_USERNAME").unwrap_or_else(|_| "test@example.com".to_string());
    let password = env::var("MOODLE_PASSWORD").unwrap_or_else(|_| "password123".to_string());
    let is_live = env::var("LIVE_TESTS").map(|v| v == "1" || v.to_lowercase() == "true").unwrap_or(false);

    if is_live {
        // Clean environment variables to use production URLs
        env::remove_var("EKURSY_BASE_URL");
        env::remove_var("ELOGIN_BASE_URL");
        TestContext {
            _guard: guard,
            mock_server: None,
            is_live: true,
            email,
            password,
        }
    } else {
        // Spin up mock server
        let mock_server = MockServer::start().await;
        env::set_var("EKURSY_BASE_URL", mock_server.uri());
        env::set_var("ELOGIN_BASE_URL", mock_server.uri());
        TestContext {
            _guard: guard,
            mock_server: Some(mock_server),
            is_live: false,
            email,
            password,
        }
    }
}

#[tokio::test]
async fn test_login_success() {
    let ctx = setup_test_context().await;
    let app = get_app();

    if !ctx.is_live {
        let server = ctx.mock_server.as_ref().unwrap();

        // 1. elogin Authorize GET -> Redirects to /login_page
        Mock::given(method("GET"))
            .and(path("/"))
            .respond_with(ResponseTemplate::new(302).insert_header("Location", "/login_page"))
            .mount(server)
            .await;

        // 1b. elogin login_page GET
        Mock::given(method("GET"))
            .and(path("/login_page"))
            .respond_with(ResponseTemplate::new(200).set_body_string(
                r#"<html><body><input type="hidden" name="csrf_token" value="mock-csrf"/></body></html>"#
            ))
            .mount(server)
            .await;

        // 2. elogin Post login -> Redirects to /redirect_form
        Mock::given(method("POST"))
            .and(path("/login_page"))
            .respond_with(ResponseTemplate::new(302).insert_header("Location", "/redirect_form"))
            .mount(server)
            .await;

        // 2b. elogin redirect_form GET
        Mock::given(method("GET"))
            .and(path("/redirect_form"))
            .respond_with(ResponseTemplate::new(200).set_body_string(
                r#"<html><body>
                    <input name="version" value="2.0"/>
                    <input name="language" value="pl"/>
                    <input name="gravatar_hash" value="grav-hash"/>
                    <input name="gravatarHash" value="gravHash"/>
                    <input name="sessionAuthorizationKey" value="sess-key"/>
                </body></html>"#
            ))
            .mount(server)
            .await;

        // 3. ekursy redirect Post
        Mock::given(method("POST"))
            .and(path("/auth/ekonto/redirect.php"))
            .respond_with(ResponseTemplate::new(200))
            .mount(server)
            .await;

        // 4. ekursy login index.php Post
        Mock::given(method("POST"))
            .and(path("/login/index.php"))
            .respond_with(ResponseTemplate::new(200).insert_header("Set-Cookie", "MoodleSession=mocksessioncookie123; path=/"))
            .mount(server)
            .await;
    }

    let request = Request::builder()
        .method("POST")
        .uri("/api/login")
        .header(CONTENT_TYPE, "application/json")
        .body(Body::from(
            json!({
                "email": ctx.email,
                "password": ctx.password
            })
            .to_string(),
        ))
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body_bytes = axum::body::to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
    let body: Value = serde_json::from_slice(&body_bytes).unwrap();
    
    assert_eq!(body["msg"], "OK");
    assert!(!body["moodleSessionKey"].as_str().unwrap_or_default().is_empty());
}

#[tokio::test]
async fn test_login_invalid_credentials() {
    let ctx = setup_test_context().await;
    let app = get_app();

    if !ctx.is_live {
        let server = ctx.mock_server.as_ref().unwrap();

        // elogin Authorize GET -> Redirects to /login_page
        Mock::given(method("GET"))
            .and(path("/"))
            .respond_with(ResponseTemplate::new(302).insert_header("Location", "/login_page"))
            .mount(server)
            .await;

        // elogin login_page GET
        Mock::given(method("GET"))
            .and(path("/login_page"))
            .respond_with(ResponseTemplate::new(200).set_body_string(
                r#"<html><body><input type="hidden" name="csrf_token" value="mock-csrf"/></body></html>"#
            ))
            .mount(server)
            .await;

        // elogin Post login failure (returns 200 OK at the same /login_page url)
        Mock::given(method("POST"))
            .and(path("/login_page"))
            .respond_with(ResponseTemplate::new(200).set_body_string("login failed"))
            .mount(server)
            .await;
    }

    let request = Request::builder()
        .method("POST")
        .uri("/api/login")
        .header(CONTENT_TYPE, "application/json")
        .body(Body::from(
            json!({
                "email": "wrong@example.com",
                "password": "wrong"
            })
            .to_string(),
        ))
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    
    if ctx.is_live {
        // Real server will return UNAUTHORIZED for wrong credentials
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    } else {
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        let body_bytes = axum::body::to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
        let body: Value = serde_json::from_slice(&body_bytes).unwrap();
        assert_eq!(body["error"], "ERR-004");
    }
}

#[tokio::test]
async fn test_me_endpoint() {
    let ctx = setup_test_context().await;
    let app = get_app();

    let session_key = if ctx.is_live {
        // For live tests, perform a quick login first to get the key
        let login_app = get_app();
        let req = Request::builder()
            .method("POST")
            .uri("/api/login")
            .header(CONTENT_TYPE, "application/json")
            .body(Body::from(json!({"email": ctx.email, "password": ctx.password}).to_string()))
            .unwrap();
        let res = login_app.oneshot(req).await.unwrap();
        let bytes = axum::body::to_bytes(res.into_body(), 1024 * 1024).await.unwrap();
        let val: Value = serde_json::from_slice(&bytes).unwrap();
        val["moodleSessionKey"].as_str().unwrap().to_string()
    } else {
        let server = ctx.mock_server.as_ref().unwrap();
        Mock::given(method("GET"))
            .and(path("/user/profile.php"))
            .respond_with(ResponseTemplate::new(200).set_body_string(
                r#"<html><body>
                    <div class="contentnode"><dl>
                        <dt>Student ID</dt><dd>123456</dd>
                        <dt>Email address</dt><dd>test@example.com</dd>
                    </dl></div>
                </body></html>"#
            ))
            .mount(server)
            .await;
        "mock-session-cookie".to_string()
    };

    let request = Request::builder()
        .method("GET")
        .uri("/api/me")
        .header(AUTHORIZATION, &session_key)
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body_bytes = axum::body::to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
    let body: Value = serde_json::from_slice(&body_bytes).unwrap();

    if !ctx.is_live {
        assert_eq!(body["Student ID"], "123456");
        assert_eq!(body["Email address"], "test@example.com");
    }
}

#[tokio::test]
async fn test_me_unauthorized() {
    let _ctx = setup_test_context().await;
    let app = get_app();

    let request = Request::builder()
        .method("GET")
        .uri("/api/me")
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn test_course_list_endpoint() {
    let ctx = setup_test_context().await;
    let app = get_app();

    let session_key = if ctx.is_live {
        // Perform login to get real session key
        let login_app = get_app();
        let req = Request::builder()
            .method("POST")
            .uri("/api/login")
            .header(CONTENT_TYPE, "application/json")
            .body(Body::from(json!({"email": ctx.email, "password": ctx.password}).to_string()))
            .unwrap();
        let res = login_app.oneshot(req).await.unwrap();
        let bytes = axum::body::to_bytes(res.into_body(), 1024 * 1024).await.unwrap();
        let val: Value = serde_json::from_slice(&bytes).unwrap();
        val["moodleSessionKey"].as_str().unwrap().to_string()
    } else {
        let server = ctx.mock_server.as_ref().unwrap();

        // mock dashboard GET for sesskey
        Mock::given(method("GET"))
            .and(path("/my"))
            .respond_with(ResponseTemplate::new(200).set_body_string(
                r#"<html><body><input name="sesskey" value="mock-sesskey-val"/></body></html>"#
            ))
            .mount(server)
            .await;

        // mock ajax service POST for courses list
        Mock::given(method("POST"))
            .and(path("/lib/ajax/service.php"))
            .and(query_param("sesskey", "mock-sesskey-val"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!([
                {
                    "error": false,
                    "data": {
                        "courses": [
                            {
                                "id": 101,
                                "fullname": "Software Engineering II",
                                "shortname": "SE-2",
                                "idnumber": "SE2",
                                "summary": "",
                                "summaryformat": 1,
                                "startdate": 0,
                                "enddate": 0,
                                "visible": true,
                                "showactivitydates": false,
                                "fullnamedisplay": "Software Engineering II",
                                "viewurl": "https://ekursy.put.poznan.pl/course/view.php?id=101",
                                "courseimage": "",
                                "progress": 0,
                                "hasprogress": false,
                                "isfavourite": false,
                                "hidden": false,
                                "showshortname": false,
                                "coursecategory": "CS Department"
                            }
                        ],
                        "nextoffset": 0
                    }
                }
            ])))
            .mount(server)
            .await;
        "mock-session-cookie".to_string()
    };

    let request = Request::builder()
        .method("GET")
        .uri("/api/course_list")
        .header(AUTHORIZATION, &session_key)
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body_bytes = axum::body::to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
    let body: Value = serde_json::from_slice(&body_bytes).unwrap();
    
    assert_eq!(body["msg"], "OK");
    assert!(body["courses"].is_array());
    
    if !ctx.is_live {
        let courses = body["courses"].as_array().unwrap();
        assert_eq!(courses.len(), 1);
        assert_eq!(courses[0]["name"], "Software Engineering II");
        assert_eq!(courses[0]["id"], "101");
    }
}

#[tokio::test]
async fn test_course_content_endpoint() {
    let ctx = setup_test_context().await;
    let app = get_app();

    let session_key = if ctx.is_live {
        // Perform login to get real session key
        let login_app = get_app();
        let req = Request::builder()
            .method("POST")
            .uri("/api/login")
            .header(CONTENT_TYPE, "application/json")
            .body(Body::from(json!({"email": ctx.email, "password": ctx.password}).to_string()))
            .unwrap();
        let res = login_app.oneshot(req).await.unwrap();
        let bytes = axum::body::to_bytes(res.into_body(), 1024 * 1024).await.unwrap();
        let val: Value = serde_json::from_slice(&bytes).unwrap();
        val["moodleSessionKey"].as_str().unwrap().to_string()
    } else {
        let server = ctx.mock_server.as_ref().unwrap();

        Mock::given(method("GET"))
            .and(path("/course/view.php"))
            .and(query_param("id", "202"))
            .respond_with(ResponseTemplate::new(200).set_body_string(
                r#"<html><body>
                    <div class="course-content">
                        <a href="https://ekursy.put.poznan.pl/mod/resource/view.php?id=303">Lecture Slides</a>
                    </div>
                </body></html>"#
            ))
            .mount(server)
            .await;
        "mock-session-cookie".to_string()
    };

    let request = Request::builder()
        .method("GET")
        .uri("/api/course?id=202")
        .header(AUTHORIZATION, &session_key)
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body_bytes = axum::body::to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
    let body: Value = serde_json::from_slice(&body_bytes).unwrap();
    
    assert_eq!(body["msg"], "OK");
    assert_eq!(body["parser"], "mercury");
    assert!(body["result"].is_array());

    if !ctx.is_live {
        let result = body["result"].as_array().unwrap();
        assert_eq!(result[0]["type"], "resource");
        assert_eq!(result[0]["text"], "Lecture Slides");
        assert_eq!(result[0]["id"], "303");
        assert_eq!(result[0]["kind"], "resource");
    }
}

#[tokio::test]
async fn test_course_grades_endpoint() {
    let ctx = setup_test_context().await;
    let app = get_app();

    let session_key = if ctx.is_live {
        // Perform login to get real session key
        let login_app = get_app();
        let req = Request::builder()
            .method("POST")
            .uri("/api/login")
            .header(CONTENT_TYPE, "application/json")
            .body(Body::from(json!({"email": ctx.email, "password": ctx.password}).to_string()))
            .unwrap();
        let res = login_app.oneshot(req).await.unwrap();
        let bytes = axum::body::to_bytes(res.into_body(), 1024 * 1024).await.unwrap();
        let val: Value = serde_json::from_slice(&bytes).unwrap();
        val["moodleSessionKey"].as_str().unwrap().to_string()
    } else {
        let server = ctx.mock_server.as_ref().unwrap();

        Mock::given(method("GET"))
            .and(path("/grade/report/user/index.php"))
            .and(query_param("id", "202"))
            .respond_with(ResponseTemplate::new(200).set_body_string(
                r#"<html><body>
                    <table class="user-grade">
                        <tbody>
                            <tr>
                                <th class="column-itemname level1">
                                    <div class="category-content"><span>Grade Category</span></div>
                                </th>
                                <td class="column-weight">100.0 %</td>
                                <td class="column-grade">5.0</td>
                                <td class="column-range">2-5</td>
                                <td class="column-percentage">100.0 %</td>
                                <td class="column-lettergrade">A</td>
                                <td class="column-feedback">Excellent</td>
                                <td class="column-contributiontocoursetotal">100.0 %</td>
                            </tr>
                        </tbody>
                    </table>
                </body></html>"#
            ))
            .mount(server)
            .await;
        "mock-session-cookie".to_string()
    };

    let request = Request::builder()
        .method("GET")
        .uri("/api/course_grades?id=202")
        .header(AUTHORIZATION, &session_key)
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body_bytes = axum::body::to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
    let body: Value = serde_json::from_slice(&body_bytes).unwrap();
    assert!(body.is_array() || body.is_object());
}

#[tokio::test]
async fn test_resource_endpoint() {
    let ctx = setup_test_context().await;
    let app = get_app();

    let session_key = if ctx.is_live {
        // Perform login to get real session key
        let login_app = get_app();
        let req = Request::builder()
            .method("POST")
            .uri("/api/login")
            .header(CONTENT_TYPE, "application/json")
            .body(Body::from(json!({"email": ctx.email, "password": ctx.password}).to_string()))
            .unwrap();
        let res = login_app.oneshot(req).await.unwrap();
        let bytes = axum::body::to_bytes(res.into_body(), 1024 * 1024).await.unwrap();
        let val: Value = serde_json::from_slice(&bytes).unwrap();
        val["moodleSessionKey"].as_str().unwrap().to_string()
    } else {
        let server = ctx.mock_server.as_ref().unwrap();

        Mock::given(method("GET"))
            .and(path("/mod/url/view.php"))
            .and(query_param("id", "303"))
            .respond_with(ResponseTemplate::new(200).set_body_string(
                r#"<html><body>
                    <div class="urlworkaround">
                        <a href="https://example.com/external-redirect">Click here</a>
                    </div>
                </body></html>"#
            ))
            .mount(server)
            .await;
        "mock-session-cookie".to_string()
    };

    let request = Request::builder()
        .method("GET")
        .uri("/api/resource?id=303&kind=url")
        .header(AUTHORIZATION, &session_key)
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    if ctx.is_live {
        assert!(
            response.status() == StatusCode::INTERNAL_SERVER_ERROR
                || response.status().is_redirection()
                || response.status() == StatusCode::OK
        );
    } else {
        assert!(response.status().is_redirection() || response.status() == StatusCode::OK);
    }
}

#[tokio::test]
async fn test_proxy_endpoint() {
    let ctx = setup_test_context().await;
    let app = get_app();

    let session_key = if ctx.is_live {
        // Perform login to get real session key
        let login_app = get_app();
        let req = Request::builder()
            .method("POST")
            .uri("/api/login")
            .header(CONTENT_TYPE, "application/json")
            .body(Body::from(json!({"email": ctx.email, "password": ctx.password}).to_string()))
            .unwrap();
        let res = login_app.oneshot(req).await.unwrap();
        let bytes = axum::body::to_bytes(res.into_body(), 1024 * 1024).await.unwrap();
        let val: Value = serde_json::from_slice(&bytes).unwrap();
        val["moodleSessionKey"].as_str().unwrap().to_string()
    } else {
        let server = ctx.mock_server.as_ref().unwrap();

        Mock::given(method("GET"))
            .and(path("/pluginfile.php/123/course/legacy/file.txt"))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("Content-Type", "text/plain")
                    .set_body_string("hello from proxy"),
            )
            .mount(server)
            .await;
        "mock-session-cookie".to_string()
    };

    let request = Request::builder()
        .method("GET")
        .uri("/api/proxy?path=pluginfile.php/123/course/legacy/file.txt")
        .header(AUTHORIZATION, &session_key)
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    
    let content_type = response.headers().get(CONTENT_TYPE).unwrap().to_str().unwrap();
    assert!(content_type.starts_with("text/plain") || content_type.starts_with("text/html"));

    if !ctx.is_live {
        let body_bytes = axum::body::to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
        assert_eq!(std::str::from_utf8(&body_bytes).unwrap(), "hello from proxy");
    }
}

#[tokio::test]
async fn test_forum_endpoint() {
    let ctx = setup_test_context().await;
    let app = get_app();

    let session_key = if ctx.is_live {
        // Perform login to get real session key
        let login_app = get_app();
        let req = Request::builder()
            .method("POST")
            .uri("/api/login")
            .header(CONTENT_TYPE, "application/json")
            .body(Body::from(json!({"email": ctx.email, "password": ctx.password}).to_string()))
            .unwrap();
        let res = login_app.oneshot(req).await.unwrap();
        let bytes = axum::body::to_bytes(res.into_body(), 1024 * 1024).await.unwrap();
        let val: Value = serde_json::from_slice(&bytes).unwrap();
        val["moodleSessionKey"].as_str().unwrap().to_string()
    } else {
        let server = ctx.mock_server.as_ref().unwrap();

        Mock::given(method("GET"))
            .and(path("/mod/forum/view.php"))
            .and(query_param("id", "404"))
            .respond_with(ResponseTemplate::new(200).set_body_string(
                r#"<html><body>
                    <h2>Forum Title</h2>
                    <div id="intro"><div class="no-overflow">Forum Description</div></div>
                    <table>
                        <tr class="discussion" data-discussionid="505">
                            <th class="topic"><a href="discuss.php?d=505">Discussion Title</a></th>
                        </tr>
                    </table>
                </body></html>"#
            ))
            .mount(server)
            .await;
        "mock-session-cookie".to_string()
    };

    let request = Request::builder()
        .method("GET")
        .uri("/api/forum?id=404")
        .header(AUTHORIZATION, &session_key)
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body_bytes = axum::body::to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
    let body: Value = serde_json::from_slice(&body_bytes).unwrap();
    
    assert_eq!(body["msg"], "OK");
    assert_eq!(body["forum"]["id"], "404");
    
    if !ctx.is_live {
        assert_eq!(body["forum"]["title"], "Forum Title");
        assert_eq!(body["forum"]["description"], "Forum Description");
        assert_eq!(body["forum"]["threads"][0]["id"], "505");
        assert_eq!(body["forum"]["threads"][0]["title"], "Discussion Title");
    }
}

#[tokio::test]
async fn test_forum_discussion_endpoint() {
    let ctx = setup_test_context().await;
    let app = get_app();

    let session_key = if ctx.is_live {
        // Perform login to get real session key
        let login_app = get_app();
        let req = Request::builder()
            .method("POST")
            .uri("/api/login")
            .header(CONTENT_TYPE, "application/json")
            .body(Body::from(json!({"email": ctx.email, "password": ctx.password}).to_string()))
            .unwrap();
        let res = login_app.oneshot(req).await.unwrap();
        let bytes = axum::body::to_bytes(res.into_body(), 1024 * 1024).await.unwrap();
        let val: Value = serde_json::from_slice(&bytes).unwrap();
        val["moodleSessionKey"].as_str().unwrap().to_string()
    } else {
        let server = ctx.mock_server.as_ref().unwrap();

        Mock::given(method("GET"))
            .and(path("/mod/forum/discuss.php"))
            .and(query_param("d", "505"))
            .respond_with(ResponseTemplate::new(200).set_body_string(
                r##"<html><body>
                    <h3 class="discussionname">Discussion Title</h3>
                    <article data-region="post" data-post-id="606">
                        <div class="post-heading">Post Subject</div>
                        <div class="author-time-info">
                            <a href="#">Author Name</a>
                            <time>12:00 PM</time>
                        </div>
                        <div class="fullmessage">Post Content</div>
                    </article>
                </body></html>"##
            ))
            .mount(server)
            .await;
        "mock-session-cookie".to_string()
    };

    let request = Request::builder()
        .method("GET")
        .uri("/api/forum/discussion?id=505")
        .header(AUTHORIZATION, &session_key)
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body_bytes = axum::body::to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
    let body: Value = serde_json::from_slice(&body_bytes).unwrap();
    
    assert_eq!(body["msg"], "OK");
    assert_eq!(body["discussion"]["id"], "505");
    
    if !ctx.is_live {
        assert_eq!(body["discussion"]["title"], "Discussion Title");
        assert_eq!(body["discussion"]["posts"][0]["id"], "606");
        assert_eq!(body["discussion"]["posts"][0]["subject"], "Post Subject");
        assert_eq!(body["discussion"]["posts"][0]["author"], "Author Name");
        assert_eq!(body["discussion"]["posts"][0]["time"], "12:00 PM");
        assert_eq!(body["discussion"]["posts"][0]["content"], "<div class=\"fullmessage\">Post Content</div>");
    }
}
