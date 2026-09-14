use download_manager_engine::network::{
    EntityTag, FallbackReason, ProbeClient, ProbeError, ProbeMode, RangeAssignment,
    RangeValidationError, Validators, validate_range_response,
};
use download_manager_test_server::{
    Fault, FaultRule, Fixture, RequestSelector, ServerConfig, TestServer,
};
use reqwest::StatusCode;
use reqwest::header::{
    CONTENT_ENCODING, CONTENT_LENGTH, CONTENT_RANGE, ETAG, HeaderMap, HeaderValue, LAST_MODIFIED,
};

fn server() -> TestServer {
    TestServer::start(ServerConfig {
        fixture: Fixture {
            len: 1024,
            seed: 29,
        },
        rules: Vec::new(),
    })
    .expect("fixture should bind")
}

#[tokio::test]
async fn small_ranged_get_proves_both_resource_boundaries() {
    let server = server();
    let client = ProbeClient::new().expect("client configuration should be valid");
    let probe = client
        .probe(&server.url("/fixture"))
        .await
        .expect("correct fixture should probe");

    assert_eq!(probe.mode(), ProbeMode::Segmented);
    assert_eq!(probe.size(), Some(1024));
    assert_eq!(probe.filename(), Some("fixture"));
    assert_eq!(probe.final_url().as_str(), server.url("/fixture"));
    assert!(probe.validators().etag.is_some());
    assert!(probe.validators().last_modified.is_some());

    let requests = server.requests();
    assert_eq!(requests.len(), 2);
    assert_eq!(
        requests[0].range.map(|value| (value.start, value.end)),
        Some((0, 0))
    );
    assert_eq!(
        requests[1].range.map(|value| (value.start, value.end)),
        Some((1023, 1023))
    );
}

#[tokio::test]
async fn ignored_ranges_fall_back_without_consuming_the_full_probe_body() {
    let server = server();
    let client = ProbeClient::new().expect("client configuration should be valid");

    let known = client
        .probe(&server.url("/ignore-range"))
        .await
        .expect("ignored range with length should safely fall back");
    assert_eq!(
        known.mode(),
        ProbeMode::SingleStream(FallbackReason::RangeIgnored)
    );
    assert_eq!(known.size(), Some(1024));

    let unknown = client
        .probe(&server.url("/unknown-length"))
        .await
        .expect("unknown length should safely fall back");
    assert_eq!(
        unknown.mode(),
        ProbeMode::SingleStream(FallbackReason::UnknownLength)
    );
    assert_eq!(unknown.size(), None);
}

#[tokio::test]
async fn malformed_ranges_and_resource_inconsistency_are_rejected() {
    let server = server();
    let client = ProbeClient::new().expect("client configuration should be valid");

    for path in [
        "/bad-range/start",
        "/bad-range/end",
        "/bad-range/total",
        "/validators/changing",
        "/mutating",
    ] {
        let result = client.probe(&server.url(path)).await;
        assert!(result.is_err(), "unsafe fixture {path} was accepted");
    }

    let encoded_result = client.probe(&server.url("/encoded")).await;
    assert_eq!(
        encoded_result,
        Err(ProbeError::InvalidRange(
            RangeValidationError::UnexpectedContentEncoding
        )),
        "observed requests: {:?}",
        server.requests()
    );
}

#[tokio::test]
async fn redirect_policy_resolves_once_and_rejects_loops() {
    let server = server();
    let client = ProbeClient::new().expect("client configuration should be valid");

    let redirected = client
        .probe(&server.url("/redirect/once"))
        .await
        .expect("one HTTP redirect should resolve");
    assert_eq!(redirected.final_url().as_str(), server.url("/fixture"));
    assert_eq!(redirected.mode(), ProbeMode::Segmented);

    let loop_result = client.probe(&server.url("/redirect/loop-a")).await;
    assert_eq!(
        loop_result,
        Err(ProbeError::RedirectRejected),
        "observed requests: {:?}",
        server.requests()
    );

    let user_info_server = TestServer::start(ServerConfig {
        fixture: Fixture { len: 8, seed: 2 },
        rules: vec![FaultRule {
            selector: RequestSelector {
                path: Some("/fixture".to_owned()),
                request_number: Some(1),
                range: None,
            },
            fault: Fault::Redirect("http://user:password@127.0.0.1:1/file".to_owned()),
        }],
    })
    .expect("fixture should bind");
    assert_eq!(
        client.probe(&user_info_server.url("/fixture")).await,
        Err(ProbeError::RedirectRejected)
    );
}

#[tokio::test]
async fn statuses_and_retry_after_are_bounded_machine_data() {
    let server = server();
    let client = ProbeClient::new().expect("client configuration should be valid");

    for (path, expected) in [
        ("/status/403", 403),
        ("/status/404", 404),
        ("/status/416", 416),
    ] {
        assert_eq!(
            client.probe(&server.url(path)).await,
            Err(ProbeError::HttpStatus {
                status: expected,
                retry_after_seconds: None,
            })
        );
    }
    assert_eq!(
        client.probe(&server.url("/status/429")).await,
        Err(ProbeError::HttpStatus {
            status: 429,
            retry_after_seconds: Some(2),
        })
    );
    assert_eq!(
        client.probe(&server.url("/status/503")).await,
        Err(ProbeError::HttpStatus {
            status: 503,
            retry_after_seconds: Some(1),
        })
    );
}

#[tokio::test]
async fn url_boundary_rejects_unsupported_schemes_and_user_info() {
    let client = ProbeClient::new().expect("client configuration should be valid");
    assert_eq!(
        client.probe("file:///C:/secret").await,
        Err(ProbeError::UnsupportedScheme)
    );
    assert_eq!(client.probe("not a URL").await, Err(ProbeError::InvalidUrl));
    assert_eq!(
        client
            .probe("https://user:password@example.test/file")
            .await,
        Err(ProbeError::UserInfoForbidden)
    );
}

#[tokio::test]
async fn short_probe_body_is_never_accepted() {
    let server = TestServer::start(ServerConfig {
        fixture: Fixture { len: 8, seed: 2 },
        rules: vec![FaultRule {
            selector: RequestSelector {
                path: Some("/fixture".to_owned()),
                request_number: Some(1),
                range: None,
            },
            fault: Fault::DisconnectAfter(0),
        }],
    })
    .expect("fixture should bind");
    let client = ProbeClient::new().expect("client configuration should be valid");

    assert_eq!(
        client.probe(&server.url("/fixture")).await,
        Err(ProbeError::BodyLengthMismatch)
    );
}

fn valid_headers() -> HeaderMap {
    let mut headers = HeaderMap::new();
    headers.insert(CONTENT_RANGE, HeaderValue::from_static("bytes 10-19/100"));
    headers.insert(CONTENT_LENGTH, HeaderValue::from_static("10"));
    headers.insert(ETAG, HeaderValue::from_static("\"stable\""));
    headers.insert(
        LAST_MODIFIED,
        HeaderValue::from_static("Mon, 01 Jan 2024 00:00:00 GMT"),
    );
    headers
}

fn expected_validators() -> Validators {
    Validators {
        etag: Some(EntityTag::parse("\"stable\"").expect("test ETag should be valid")),
        last_modified: Some("Mon, 01 Jan 2024 00:00:00 GMT".to_owned()),
    }
}

#[test]
fn worker_validation_requires_exact_status_range_length_and_identity() {
    let assignment = RangeAssignment::new(10, 19, 100).expect("assignment should be valid");
    let expected = expected_validators();
    assert!(
        validate_range_response(
            StatusCode::PARTIAL_CONTENT,
            &valid_headers(),
            assignment,
            &expected,
        )
        .is_ok()
    );

    assert_eq!(
        validate_range_response(StatusCode::OK, &valid_headers(), assignment, &expected),
        Err(RangeValidationError::WrongStatus)
    );

    let mut cases = Vec::new();
    let mut missing = valid_headers();
    missing.remove(CONTENT_RANGE);
    cases.push((missing, RangeValidationError::MissingHeader));
    let mut wrong_start = valid_headers();
    wrong_start.insert(CONTENT_RANGE, HeaderValue::from_static("bytes 11-19/100"));
    cases.push((wrong_start, RangeValidationError::ContentRangeMismatch));
    let mut wrong_end = valid_headers();
    wrong_end.insert(CONTENT_RANGE, HeaderValue::from_static("bytes 10-20/100"));
    cases.push((wrong_end, RangeValidationError::ContentRangeMismatch));
    let mut wrong_total = valid_headers();
    wrong_total.insert(CONTENT_RANGE, HeaderValue::from_static("bytes 10-19/101"));
    cases.push((wrong_total, RangeValidationError::TotalMismatch));
    let mut wrong_length = valid_headers();
    wrong_length.insert(CONTENT_LENGTH, HeaderValue::from_static("11"));
    cases.push((wrong_length, RangeValidationError::ContentLengthMismatch));
    let mut encoded = valid_headers();
    encoded.insert(CONTENT_ENCODING, HeaderValue::from_static("gzip"));
    cases.push((encoded, RangeValidationError::UnexpectedContentEncoding));

    for (headers, error) in cases {
        assert_eq!(
            validate_range_response(StatusCode::PARTIAL_CONTENT, &headers, assignment, &expected),
            Err(error)
        );
    }
}

#[test]
fn worker_validation_rejects_missing_changed_and_duplicate_validators() {
    let assignment = RangeAssignment::new(10, 19, 100).expect("assignment should be valid");
    let expected = expected_validators();

    let mut missing = valid_headers();
    missing.remove(ETAG);
    assert_eq!(
        validate_range_response(StatusCode::PARTIAL_CONTENT, &missing, assignment, &expected),
        Err(RangeValidationError::ValidatorMissing)
    );

    let mut changed = valid_headers();
    changed.insert(ETAG, HeaderValue::from_static("\"changed\""));
    assert_eq!(
        validate_range_response(StatusCode::PARTIAL_CONTENT, &changed, assignment, &expected),
        Err(RangeValidationError::ValidatorChanged)
    );

    let mut duplicate = valid_headers();
    duplicate.append(CONTENT_RANGE, HeaderValue::from_static("bytes 10-19/100"));
    assert_eq!(
        validate_range_response(
            StatusCode::PARTIAL_CONTENT,
            &duplicate,
            assignment,
            &expected
        ),
        Err(RangeValidationError::DuplicateHeader)
    );
}

#[tokio::test]
async fn weak_or_missing_etag_requires_one_fresh_stream_even_with_last_modified() {
    let server = server();
    let client = ProbeClient::new().expect("client");
    for path in ["/validators/missing", "/validators/weak"] {
        let probe = client
            .probe(&server.url(path))
            .await
            .expect("fallback probe");
        assert_eq!(
            probe.mode(),
            ProbeMode::SingleStream(FallbackReason::InsufficientIdentity)
        );
        assert!(!probe.validators().has_strong_identity());
    }
}

#[tokio::test]
async fn anonymous_no_redirect_probe_refuses_before_contacting_the_target() {
    let target = server();
    let source = TestServer::start(ServerConfig {
        fixture: Fixture {
            len: 1024,
            seed: 29,
        },
        rules: vec![FaultRule {
            selector: RequestSelector {
                path: Some("/fixture".to_owned()),
                request_number: None,
                range: None,
            },
            fault: Fault::Redirect(target.url("/fixture")),
        }],
    })
    .expect("owned redirect source");
    let client = ProbeClient::new().expect("client");
    let result = client
        .probe_anonymous_without_redirects(&source.url("/fixture"))
        .await;
    let source_requests = source.requests();
    let target_requests = target.requests();
    drop(client);
    drop(source);
    drop(target);
    assert_eq!(result, Err(ProbeError::RedirectRejected));
    assert_eq!(source_requests.len(), 1);
    assert!(
        target_requests.is_empty(),
        "no native redirect context may be omitted after contact"
    );
}

#[tokio::test]
async fn anonymous_no_redirect_probe_preserves_boundary_validation_and_ordinary_following() {
    let server = server();
    let client = ProbeClient::new().expect("client");
    let direct = client
        .probe_anonymous_without_redirects(&server.url("/fixture"))
        .await;
    let bad = client
        .probe_anonymous_without_redirects(&server.url("/bad-range/start"))
        .await;
    let redirected = client
        .probe_anonymous_without_redirects(&server.url("/redirect/once"))
        .await;
    let ordinary = client.probe(&server.url("/redirect/once")).await;
    let requests = server.requests();
    let expected_url = server.url("/fixture");
    drop(client);
    drop(server);
    let direct = direct.expect("direct anonymous probe");
    assert_eq!(direct.mode(), ProbeMode::Segmented);
    assert_eq!(direct.size(), Some(1024));
    assert_eq!(direct.final_url().as_str(), expected_url);
    assert_eq!(
        requests[0].range.map(|value| (value.start, value.end)),
        Some((0, 0))
    );
    assert_eq!(
        requests[1].range.map(|value| (value.start, value.end)),
        Some((1023, 1023))
    );
    assert!(bad.is_err());
    assert_eq!(redirected, Err(ProbeError::RedirectRejected));
    assert_eq!(
        ordinary
            .expect("ordinary following retained")
            .final_url()
            .as_str(),
        expected_url
    );
    assert_eq!(requests.len(), 7);
}

#[tokio::test]
async fn anonymous_no_redirect_probe_refuses_a_redirect_at_the_final_boundary() {
    let target = server();
    let source = TestServer::start(ServerConfig {
        fixture: Fixture {
            len: 1024,
            seed: 29,
        },
        rules: vec![FaultRule {
            selector: RequestSelector {
                path: Some("/fixture".to_owned()),
                request_number: Some(2),
                range: None,
            },
            fault: Fault::Redirect(target.url("/fixture")),
        }],
    })
    .expect("owned boundary redirect");
    let client = ProbeClient::new().expect("client");
    let result = client
        .probe_anonymous_without_redirects(&source.url("/fixture"))
        .await;
    let source_requests = source.requests();
    let target_requests = target.requests();
    drop(client);
    drop(source);
    drop(target);
    assert_eq!(result, Err(ProbeError::RedirectRejected));
    assert_eq!(source_requests.len(), 2);
    assert_eq!(
        source_requests[1]
            .range
            .map(|value| (value.start, value.end)),
        Some((1023, 1023))
    );
    assert!(
        target_requests.is_empty(),
        "final-boundary redirect target must remain untouched"
    );
}
