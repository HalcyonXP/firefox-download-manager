use super::*;
use std::time::Instant;

fn connect(server: &TestServer, request: &[u8]) -> TcpStream {
    let mut stream = TcpStream::connect(server.address()).expect("owned connection");
    stream
        .set_read_timeout(Some(Duration::from_secs(2)))
        .expect("read bound");
    stream.write_all(request).expect("fixed request");
    stream
}

fn head(stream: &mut TcpStream) {
    let mut bytes = Vec::new();
    while bytes.len() < 4096 && !bytes.ends_with(b"\r\n\r\n") {
        let mut byte = [0];
        stream.read_exact(&mut byte).expect("fixture header");
        bytes.push(byte[0]);
    }
    assert!(bytes.ends_with(b"\r\n\r\n"));
}

#[test]
fn drop_waits_for_the_live_response_handler() {
    let server = TestServer::start(ServerConfig {
        rules: vec![FaultRule {
            selector: RequestSelector::default(),
            fault: Fault::Stall(Duration::from_secs(2)),
        }],
        ..ServerConfig::default()
    })
    .expect("server");
    let mut stream = connect(&server, b"GET /fixture HTTP/1.1\r\nHost: localhost\r\n\r\n");
    head(&mut stream);
    let state = Arc::clone(&server.state);
    drop(server);
    let active_at_return = lock_state(&state).active_requests;
    drop(stream);
    // Also contain the old detached-handler counterexample before asserting:
    // that handler's fixed two-second stall finishes, then its closed peer
    // releases the activity guard. This is not an old per-handler join claim.
    while lock_state(&state).active_requests != 0 {
        thread::sleep(Duration::from_millis(1));
    }
    assert_eq!(
        active_at_return, 0,
        "fixture drop returned with an active handler"
    );
}

#[test]
fn incomplete_headers_are_interrupted_and_all_retained_threads_join() {
    let mut server = TestServer::start(ServerConfig::default()).expect("server");
    let stream = connect(&server, b"GET /fixture HTTP/1.1\r\n");
    let deadline = Instant::now() + Duration::from_secs(2);
    while server.connection_counts().0 == 0 && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(1));
    }
    let started = server.connection_counts().0;
    let joined = server.retire();
    let repeated = server.retire();
    let counts = server.connection_counts();
    drop(stream);
    assert_eq!(started, 1);
    assert_eq!(joined.expect("retired"), 1);
    assert_eq!(repeated.expect("memoized"), 1);
    assert_eq!(counts, (1, 1));
}

#[test]
fn delayed_ledger_insertion_is_not_a_new_client_request() {
    let mut server = TestServer::start(ServerConfig::default()).expect("server");
    let pause = server.pause_observation();
    let stream = connect(&server, b"GET /fixture HTTP/1.1\r\nHost: localhost\r\n\r\n");
    let received = pause.wait_for_pending(1, Duration::from_secs(2));
    let closed = stream.shutdown(Shutdown::Both);
    drop(stream); // The sole client has already sent and closed, before ledger insertion.
    let before = server.requests().len();
    let joined = server.retire(); // Releases the observation gate and joins its writer.
    let after = server.requests().len();
    drop(pause);
    assert!(received && closed.is_ok());
    assert_eq!(joined.expect("retired"), 1);
    assert_eq!((before, after), (0, 1));
    assert_eq!(server.connection_counts(), (1, 1));
}

#[test]
fn response_barrier_is_released_before_handler_join() {
    let mut server = TestServer::start(ServerConfig::default()).expect("server");
    let pause = server
        .pause_responses(RequestSelector::default())
        .expect("response pause");
    let stream = connect(&server, b"GET /fixture HTTP/1.1\r\nHost: localhost\r\n\r\n");
    let received = pause.wait_for_pending(1, Duration::from_secs(2));
    let joined = server.retire();
    drop(stream);
    drop(pause);
    assert!(received);
    assert_eq!(joined.expect("retired"), 1);
    assert_eq!(server.connection_counts(), (1, 1));
}

#[test]
fn stopped_body_generation_does_not_depend_on_socket_shutdown_succeeding() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("owned listener");
    let mut client =
        TcpStream::connect(listener.local_addr().expect("address")).expect("owned client");
    client
        .set_read_timeout(Some(Duration::from_secs(2)))
        .expect("read bound");
    let (mut socket, _) = listener.accept().expect("owned connection");
    let stop = Stop::default();
    stop.request(); // Deliberately do not call socket.shutdown here.
    let result = write_generated_body(
        &mut socket,
        &Fixture::default(),
        ByteRange {
            start: 0,
            end: 1023,
        },
        0,
        None,
        &stop,
    );
    drop(socket);
    let mut bytes = Vec::new();
    let received = client.read_to_end(&mut bytes);
    drop(client);
    assert!(result.is_ok() && received.is_ok());
    assert_eq!(bytes.len(), 0);
}
