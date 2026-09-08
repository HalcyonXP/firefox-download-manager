use std::sync::Arc;

use download_manager_engine::auth::{ContextError, RequestContext};
use download_manager_engine::network::{ProbeClient, ProbeError};
use download_manager_protocol::RequestContextInput;
use download_manager_test_server::{Fault, FaultRule, RequestSelector, ServerConfig, TestServer};
use reqwest::Url;
use serde_json::{Value, json};

fn input() -> Value {
    json!({"credentials":{"cookies":[{"name":"session", "value":"synthetic-cookie", "domain":"example.test", "path":"/private", "secure":true, "http_only":true, "expires_at":null}]}, "referrer":"https://example.test/private/page"})
}

fn context(value: Value) -> Result<Arc<RequestContext>, ContextError> {
    let input: RequestContextInput = serde_json::from_value(value).expect("fixture shape");
    RequestContext::new(
        "https://example.test/private/file?sig=a%2Fb%2BC&x=2&x=1",
        &input,
    )
}

#[test]
fn scopes_cookie_path_origin_secure_and_httponly_without_debug_disclosure() {
    let context = context(input()).expect("valid HttpOnly session");
    let headers = context
        .headers(&Url::parse("https://example.test/private/file").expect("URL"))
        .expect("headers");
    assert_eq!(headers["cookie"], "session=synthetic-cookie");
    assert!(headers["cookie"].is_sensitive());
    assert!(headers["referer"].is_sensitive());
    for target in [
        "https://other.example.test/private/file",
        "https://example.test:444/private/file",
        "http://example.test/private/file",
    ] {
        assert_eq!(
            context.headers(&Url::parse(target).expect("URL")),
            Err(ContextError::Invalid)
        );
    }
    for path in ["/private-other", "/outside"] {
        let headers = context
            .headers(&Url::parse(&format!("https://example.test{path}")).expect("URL"))
            .expect("same origin");
        assert!(!headers.contains_key("cookie"));
    }
    assert!(!format!("{context:?} {headers:?}").contains("synthetic-cookie"));
    assert!(!format!("{context:?} {headers:?}").contains("/private"));
}

#[test]
fn rejects_cookie_injection_wrong_scope_missing_expiry_and_excessive_headers() {
    for (field, value) in [
        ("name", json!("bad=name")),
        ("name", json!("bad\r\nheader")),
        ("value", json!("a;b=c")),
        ("value", json!("a\nInjected: yes")),
        ("value", json!("x".repeat(8193))),
        ("domain", json!("other.test")),
        ("domain", json!("example.test.evil.test")),
        ("path", json!("/elsewhere")),
        ("expires_at", json!("not-a-date")),
    ] {
        let mut value_input = input();
        value_input["credentials"]["cookies"][0][field] = value;
        assert_eq!(
            context(value_input),
            Err(ContextError::Invalid),
            "case {field}"
        );
    }
    let mut value = input();
    value["credentials"]["cookies"][0]
        .as_object_mut()
        .expect("cookie")
        .remove("expires_at");
    assert_eq!(context(value), Err(ContextError::Invalid));
    let cookie = input()["credentials"]["cookies"][0].clone();
    assert_eq!(
        context(json!({"credentials":{"cookies":vec![cookie;257]}})),
        Err(ContextError::Invalid)
    );
}

#[test]
fn domain_cookie_is_valid_only_for_initial_target_and_expiry_uses_utc_offsets() {
    let mut value = input();
    value["credentials"]["cookies"][0]["domain"] = json!(".example.test");
    value["credentials"]["cookies"][0]["expires_at"] = json!("2099-01-01T02:00:00+02:00");
    assert!(context(value.clone()).is_ok());
    value["credentials"]["cookies"][0]["expires_at"] = json!("2000-01-01T00:00:00-04:00");
    assert_eq!(context(value), Err(ContextError::Expired));
    let parsed: RequestContextInput = serde_json::from_value(input()).expect("input");
    assert_eq!(
        RequestContext::new("http://example.test/private/file", &parsed),
        Err(ContextError::Invalid)
    );
}

#[test]
fn authorization_is_https_only_bounded_and_never_debug_formatted() {
    for scheme in ["Basic", "Bearer"] {
        let value = json!({"credentials":{"authorization":{"scheme":scheme,"value":"synthetic-authorization"}}});
        let context = context(value.clone()).expect("HTTPS authorization");
        let headers = context
            .headers(&Url::parse("https://example.test/private").expect("URL"))
            .expect("headers");
        assert_eq!(
            headers["authorization"],
            format!("{scheme} synthetic-authorization")
        );
        assert!(!format!("{context:?} {headers:?}").contains("synthetic-authorization"));
        let input: RequestContextInput = serde_json::from_value(value).expect("input");
        assert_eq!(
            RequestContext::new("http://example.test/private", &input),
            Err(ContextError::Invalid)
        );
    }
    for scheme in ["Digest", "Negotiate", "Bearer\n"] {
        assert_eq!(
            context(json!({"credentials":{"authorization":{"scheme":scheme,"value":"fake"}}})),
            Err(ContextError::Invalid)
        );
    }
    let mut value = input();
    value["referrer"] = json!("https://other.test/page");
    assert_eq!(context(value), Err(ContextError::Invalid));
}

fn session_for(server: &TestServer, target: &str) -> Arc<RequestContext> {
    let input: RequestContextInput = serde_json::from_value(json!({
        "referrer":server.url("/session/page"),"credentials":{"cookies":[{
            "name":"fixture_session","value":"not-a-real-session","domain":"127.0.0.1", "path":"/session",
            "secure":false,"http_only":true,"expires_at":null
        }]}
    })).expect("fixture context");
    RequestContext::new(target, &input).expect("validated fixture session")
}

#[tokio::test]
async fn signed_probe_transmits_applicable_session_and_exact_query_on_both_probes() {
    let server = TestServer::start(ServerConfig::default()).expect("server");
    let url = server.url("/session/signed?sig=a%2Fb%2BC&x=2&x=1");
    ProbeClient::new()
        .expect("client")
        .probe_with_context(&url, Some(session_for(&server, &url)))
        .await
        .expect("authenticated probe");
    let requests = server.requests();
    assert_eq!(requests.len(), 2);
    assert!(
        requests
            .iter()
            .all(|r| r.session.fixture_valid && r.session.signed_target_valid)
    );
    assert!(!format!("{requests:?}").contains("not-a-real-session"));
}

#[tokio::test]
async fn redirect_recomputes_paths_and_blocks_origin_change_before_contact() {
    let foreign = TestServer::start(ServerConfig::default()).expect("foreign server");
    let server = TestServer::start(ServerConfig {
        rules: vec![
            FaultRule {
                selector: RequestSelector {
                    path: Some("/session/same".into()),
                    ..RequestSelector::default()
                },
                fault: Fault::Redirect("/fixture".into()),
            },
            FaultRule {
                selector: RequestSelector {
                    path: Some("/session/cross".into()),
                    ..RequestSelector::default()
                },
                fault: Fault::Redirect(foreign.url("/fixture")),
            },
        ],
        ..ServerConfig::default()
    })
    .expect("redirect server");
    let client = ProbeClient::new().expect("client");
    let same = server.url("/session/same");
    client
        .probe_with_context(&same, Some(session_for(&server, &same)))
        .await
        .expect("same-origin redirect");
    let requests = server.requests();
    assert!(requests[0].session.cookie_present);
    assert!(requests.iter().skip(1).all(|r| !r.session.cookie_present));
    let cross = server.url("/session/cross");
    assert_eq!(
        client
            .probe_with_context(&cross, Some(session_for(&server, &cross)))
            .await,
        Err(ProbeError::RedirectRejected)
    );
    assert!(foreign.requests().is_empty());
}
