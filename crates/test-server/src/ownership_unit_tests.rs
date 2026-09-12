use super::*;
use std::net::TcpListener;
use std::sync::atomic::AtomicUsize;
use std::sync::mpsc;
use std::time::Instant;

fn pair() -> (TcpStream, TcpStream) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("owned listener");
    let client = TcpStream::connect(listener.local_addr().expect("address")).expect("owned client");
    let (server, _) = listener.accept().expect("owned accepted socket");
    (client, server)
}

fn owners() -> Connections {
    Connections::new(
        Arc::new(Mutex::new(SharedState::default())),
        Arc::new(Stop::default()),
        Arc::new(ObservationGate::default()),
        Arc::new(ResponseGate::default()),
    )
}

#[test]
fn finish_waits_beyond_socket_shutdown_for_the_handler_tail() {
    let mut owners = owners();
    let (client, socket) = pair();
    let (entered, ready) = mpsc::channel();
    let complete = Arc::new(AtomicBool::new(false));
    let observed = Arc::clone(&complete);
    let stop = Arc::clone(&owners.stop);
    owners
        .spawn(socket, move |_socket| {
            let _ = entered.send(());
            stop.wait(Duration::from_secs(30));
            thread::sleep(Duration::from_millis(50)); // A tail which socket shutdown cannot join.
            observed.store(true, Ordering::Release);
        })
        .expect("owned thread");
    let entered = ready.recv_timeout(Duration::from_secs(2)).is_ok();
    let joined = owners.finish();
    let complete_at_return = complete.load(Ordering::Acquire);
    // Mutation failure containment: never leave the deliberately unjoined tail
    // running when asserting the missing-join counterexample.
    while !complete.load(Ordering::Acquire) {
        thread::sleep(Duration::from_millis(1));
    }
    drop(client);
    assert!(entered && complete_at_return);
    assert_eq!(joined.expect("joined"), 1);
    assert_eq!(owners.finish().expect("memoized"), 1);
}

#[test]
fn failed_thread_join_is_sticky_and_does_not_skip_a_live_sibling() {
    let mut owners = owners();
    let (client, socket) = pair();
    owners
        .spawn(socket, |_| panic!("owned handler fault"))
        .expect("owned panic fixture");
    let (other, socket) = pair();
    let finished = Arc::new(AtomicUsize::new(0));
    let observed = Arc::clone(&finished);
    let stop = Arc::clone(&owners.stop);
    owners
        .spawn(socket, move |_| {
            stop.wait(Duration::from_secs(30));
            observed.fetch_add(1, Ordering::AcqRel);
        })
        .expect("owned sibling");
    let result = owners.finish();
    let repeated = owners.finish();
    let count = lock_state(&owners.state).connections_joined;
    drop((client, other));
    assert!(result.is_err() && repeated.is_err());
    assert_eq!(finished.load(Ordering::Acquire), 1);
    assert_eq!(count, 2);
}

#[test]
fn listener_unwind_retires_already_retained_handlers() {
    let mut owners = owners();
    let state = Arc::clone(&owners.state);
    let (client, socket) = pair();
    let stop = Arc::clone(&owners.stop);
    let cancellation = Arc::clone(&stop);
    let finished = Arc::new(AtomicBool::new(false));
    let observed = Arc::clone(&finished);
    owners
        .spawn(socket, move |_| {
            stop.wait(Duration::from_secs(30));
            observed.store(true, Ordering::Release);
        })
        .expect("owned handler");
    // Only this owned fault fixture is moved across the unwind boundary; its
    // Drop joins all retained handles before the shared counters are inspected.
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
        let _owner = owners;
        panic!("owned listener fault");
    }));
    let counts = {
        let state = lock_state(&state);
        (state.connections_started, state.connections_joined)
    };
    // Independent signal/tail observation contains a missing-Drop mutation; it
    // cannot retroactively supply the omitted handler join.
    cancellation.request();
    while !finished.load(Ordering::Acquire) {
        thread::sleep(Duration::from_millis(1));
    }
    drop(client);
    assert!(outcome.is_err());
    assert_eq!(counts, (1, 1));
}

#[test]
fn completed_handlers_are_reaped_without_waiting_for_server_stop() {
    let mut owners = owners();
    let (client, socket) = pair();
    owners.spawn(socket, |_| {}).expect("owned handler");
    let deadline = Instant::now() + Duration::from_secs(2);
    while !owners.entries.is_empty() && Instant::now() < deadline {
        owners.reap().expect("successful reap");
        thread::sleep(Duration::from_millis(1));
    }
    let reaped = owners.entries.is_empty();
    let joined = owners.finish();
    drop(client);
    assert!(reaped);
    assert_eq!(joined.expect("retired"), 1);
}
