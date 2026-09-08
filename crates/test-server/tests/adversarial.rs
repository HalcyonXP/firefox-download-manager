use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::thread;
use std::time::{Duration, Instant};

use download_manager_test_server::{
    ByteRange, Fault, FaultRule, Fixture, RequestSelector, ServerConfig, TestServer,
};

#[derive(Debug)]
struct Response {
    status: u16,
    headers: HashMap<String, String>,
    body: Vec<u8>,
    elapsed: Duration,
}

fn start_server(rules: Vec<FaultRule>) -> (TestServer, Fixture) {
    let fixture = Fixture {
        len: 1024,
        seed: 19,
    };
    let server = TestServer::start(ServerConfig {
        fixture: fixture.clone(),
        rules,
    })
    .expect("test server should bind to loopback");
    (server, fixture)
}

fn get(server: &TestServer, path: &str, range: Option<ByteRange>) -> Response {
    get_at_address(server.address(), path, range)
}

fn get_at_address(address: SocketAddr, path: &str, range: Option<ByteRange>) -> Response {
    let started = Instant::now();
    let mut stream = TcpStream::connect(address).expect("fixture should accept a connection");
    stream
        .set_read_timeout(Some(Duration::from_secs(3)))
        .expect("fixture timeout should configure");
    let range_header = range.map_or_else(String::new, |value| {
        format!("Range: bytes={}-{}\r\n", value.start, value.end)
    });
    write!(
        stream,
        "GET {path} HTTP/1.1\r\nHost: {address}\r\nAccept-Encoding: identity\r\n{range_header}Connection: close\r\n\r\n"
    )
    .expect("fixture request should write");

    let mut raw = Vec::new();
    stream
        .read_to_end(&mut raw)
        .expect("fixture response should be readable");
    let elapsed = started.elapsed();
    let split = raw
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .expect("response should contain a complete head");
    let head = std::str::from_utf8(&raw[..split]).expect("response head should be UTF-8");
    let mut lines = head.split("\r\n");
    let status = lines
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .and_then(|value| value.parse().ok())
        .expect("response should have a numeric status");
    let headers = lines
        .filter_map(|line| line.split_once(':'))
        .map(|(name, value)| (name.to_ascii_lowercase(), value.trim().to_owned()))
        .collect();

    Response {
        status,
        headers,
        body: raw[split + 4..].to_vec(),
        elapsed,
    }
}

fn range(start: u64, end: u64) -> ByteRange {
    ByteRange::new(start, end).expect("test range should be valid")
}

#[test]
fn payload_is_reproducible_and_independently_verifiable() {
    let fixture = Fixture { len: 512, seed: 41 };
    let bytes = fixture.bytes(200, 128, 3);
    let independent: Vec<u8> = (200_u64..328)
        .map(|offset| {
            let value = (offset
                .wrapping_mul(31)
                .wrapping_add((offset >> 8).wrapping_mul(17))
                .wrapping_add(41)
                .wrapping_add(3 * 13))
                % 251;
            value.to_le_bytes()[0]
        })
        .collect();
    assert_eq!(bytes, independent);
}

#[test]
fn serves_exact_validated_ranges_and_unsatisfied_ranges() {
    let (server, fixture) = start_server(Vec::new());
    let response = get(&server, "/fixture", Some(range(100, 199)));

    assert_eq!(response.status, 206);
    assert_eq!(
        response.headers.get("content-range").map(String::as_str),
        Some("bytes 100-199/1024")
    );
    assert_eq!(
        response.headers.get("content-length").map(String::as_str),
        Some("100")
    );
    assert_eq!(response.body, fixture.bytes(100, 100, 0));
    assert!(response.headers.contains_key("etag"));
    assert!(response.headers.contains_key("last-modified"));

    let unsatisfied = get(&server, "/fixture", Some(range(1024, 1030)));
    assert_eq!(unsatisfied.status, 416);
    assert_eq!(
        unsatisfied.headers.get("content-range").map(String::as_str),
        Some("bytes */1024")
    );

    let empty = get(&server, "/empty", Some(range(0, 0)));
    assert_eq!(empty.status, 416);
    assert_eq!(
        empty.headers.get("content-range").map(String::as_str),
        Some("bytes */0")
    );
    assert!(empty.body.is_empty());
}

#[test]
fn serves_concurrent_disjoint_ranges() {
    let (server, fixture) = start_server(Vec::new());
    let responses = thread::scope(|scope| {
        let handles: Vec<_> = (0_u64..8)
            .map(|worker| {
                let server = &server;
                scope.spawn(move || {
                    let start = worker * 128;
                    (
                        start,
                        get(server, "/fixture", Some(range(start, start + 127))),
                    )
                })
            })
            .collect();
        handles
            .into_iter()
            .map(|handle| handle.join().expect("range worker should not panic"))
            .collect::<Vec<_>>()
    });

    for (start, response) in responses {
        assert_eq!(response.status, 206);
        assert_eq!(response.body, fixture.bytes(start, 128, 0));
    }
    assert_eq!(server.requests().len(), 8);
}

#[test]
fn reproduces_ignored_and_malformed_range_responses() {
    let (server, fixture) = start_server(Vec::new());
    let ignored = get(&server, "/ignore-range", Some(range(10, 19)));
    assert_eq!(ignored.status, 200);
    assert!(!ignored.headers.contains_key("content-range"));
    assert_eq!(ignored.body, fixture.bytes(0, 1024, 0));

    for (path, expected) in [
        ("/bad-range/start", "bytes 11-19/1024"),
        ("/bad-range/end", "bytes 10-20/1024"),
        ("/bad-range/total", "bytes 10-19/1025"),
    ] {
        let response = get(&server, path, Some(range(10, 19)));
        assert_eq!(response.status, 206);
        assert_eq!(
            response.headers.get("content-range").map(String::as_str),
            Some(expected)
        );
        assert_eq!(response.body, fixture.bytes(10, 10, 0));
    }
}

#[test]
fn reproduces_missing_changing_and_mutating_resources() {
    let (server, _) = start_server(Vec::new());
    let missing = get(&server, "/validators/missing", Some(range(0, 31)));
    assert!(!missing.headers.contains_key("etag"));
    assert!(!missing.headers.contains_key("last-modified"));

    let validator_one = get(&server, "/validators/changing", Some(range(0, 31)));
    let validator_two = get(&server, "/validators/changing", Some(range(0, 31)));
    assert_ne!(
        validator_one.headers.get("etag"),
        validator_two.headers.get("etag")
    );
    assert_eq!(validator_one.body, validator_two.body);

    let mutation_one = get(&server, "/mutating", Some(range(0, 31)));
    let mutation_two = get(&server, "/mutating", Some(range(0, 31)));
    assert_ne!(
        mutation_one.headers.get("etag"),
        mutation_two.headers.get("etag")
    );
    assert_ne!(mutation_one.body, mutation_two.body);
}

#[test]
fn reproduces_redirects_statuses_and_retry_guidance() {
    let (server, _) = start_server(Vec::new());
    for (path, location) in [
        ("/redirect/once", "/fixture"),
        ("/redirect/loop-a", "/redirect/loop-b"),
        ("/redirect/loop-b", "/redirect/loop-a"),
    ] {
        let response = get(&server, path, None);
        assert_eq!(response.status, 302);
        assert_eq!(
            response.headers.get("location").map(String::as_str),
            Some(location)
        );
    }

    for (path, status) in [
        ("/status/403", 403),
        ("/status/404", 404),
        ("/status/416", 416),
        ("/status/429", 429),
        ("/status/503", 503),
    ] {
        assert_eq!(get(&server, path, None).status, status);
    }
    assert_eq!(
        get(&server, "/status/429", None)
            .headers
            .get("retry-after")
            .map(String::as_str),
        Some("2")
    );
    assert_eq!(
        get(&server, "/status/503", None)
            .headers
            .get("retry-after")
            .map(String::as_str),
        Some("1")
    );
}

#[test]
fn reproduces_disconnect_stall_unknown_length_and_encoding() {
    let (server, fixture) = start_server(Vec::new());
    let disconnect = get(&server, "/disconnect", None);
    assert_eq!(disconnect.status, 200);
    assert_eq!(
        disconnect.headers.get("content-length").map(String::as_str),
        Some("1024")
    );
    assert_eq!(disconnect.body.len(), 17);

    let stalled = get(&server, "/stall", Some(range(0, 9)));
    assert!(stalled.elapsed >= Duration::from_millis(80));
    assert_eq!(stalled.body, fixture.bytes(0, 10, 0));

    let unknown = get(&server, "/unknown-length", None);
    assert!(!unknown.headers.contains_key("content-length"));
    assert_eq!(unknown.body, fixture.bytes(0, 1024, 0));

    let encoded = get(&server, "/encoded", Some(range(0, 9)));
    assert_eq!(
        encoded.headers.get("content-encoding").map(String::as_str),
        Some("gzip")
    );
}

#[test]
fn custom_faults_target_selected_requests_and_ranges() {
    let rules = vec![
        FaultRule {
            selector: RequestSelector {
                path: Some("/fixture".to_owned()),
                request_number: Some(2),
                range: None,
            },
            fault: Fault::Status {
                code: 503,
                retry_after_seconds: Some(7),
            },
        },
        FaultRule {
            selector: RequestSelector {
                path: Some("/fixture".to_owned()),
                request_number: None,
                range: Some(range(100, 109)),
            },
            fault: Fault::IgnoreRange,
        },
    ];
    let (server, fixture) = start_server(rules);

    assert_eq!(get(&server, "/fixture", Some(range(0, 9))).status, 206);
    let selected_request = get(&server, "/fixture", Some(range(10, 19)));
    assert_eq!(selected_request.status, 503);
    assert_eq!(
        selected_request
            .headers
            .get("retry-after")
            .map(String::as_str),
        Some("7")
    );
    let selected_range = get(&server, "/fixture", Some(range(100, 109)));
    assert_eq!(selected_range.status, 200);
    assert_eq!(selected_range.body, fixture.bytes(0, 1024, 0));

    let requests = server.requests();
    assert_eq!(requests.len(), 3);
    assert_eq!(requests[1].request_number, 2);
    assert_eq!(requests[2].range, Some(range(100, 109)));
}

#[test]
fn selected_response_pause_preserves_observation_and_releases_on_guard_or_server_drop() {
    for release_server in [false, true] {
        let (server, fixture) = start_server(Vec::new());
        let range = ByteRange { start: 0, end: 15 };
        let selector = RequestSelector {
            path: Some("/fixture".to_owned()),
            request_number: None,
            range: Some(range),
        };
        let pause = server
            .pause_responses(selector.clone())
            .expect("pause selected response");
        assert_eq!(
            server
                .pause_responses(selector)
                .expect_err("reject overlapping pause")
                .kind(),
            std::io::ErrorKind::WouldBlock
        );
        assert!(!pause.wait_for_pending(1, Duration::ZERO));
        let address = server.address();
        let reader = thread::spawn(move || get_at_address(address, "/fixture", Some(range)));
        assert!(pause.wait_for_pending(1, Duration::from_secs(3)));
        assert!(
            !reader.is_finished(),
            "response cannot finish before explicit release"
        );
        assert_eq!(server.requests().len(), 1);
        assert_eq!(server.requests()[0].range, Some(range));
        // A nonmatching full GET still works while the selected response is held.
        let other = get(&server, "/fixture", None);
        assert_eq!(other.status, 200);
        assert_eq!(other.body, fixture.bytes(0, 1024, 0));
        if release_server {
            drop(server);
        } else {
            drop(pause);
        }
        let result = reader.join().expect("owned response reader");
        assert_eq!(result.status, 206);
        assert_eq!(result.body, fixture.bytes(0, 16, 0));
    }
}
