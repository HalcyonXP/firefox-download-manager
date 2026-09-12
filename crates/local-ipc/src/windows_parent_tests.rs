use super::*;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn real_parent_and_ordinary_proof_domains_preserve_frames_and_release_reservations() {
    for allow_parent in [false, true] {
        for class in [PeerClass::NativeBridge, PeerClass::BrowserParent] {
            let endpoint = Endpoint::generate().unwrap();
            let key = Arc::new(Capability::generate().unwrap());
            let server = Server::bind(endpoint, Arc::clone(&key)).unwrap();
            let accept = async {
                if allow_parent {
                    server.accept_with_browser_parent().await
                } else {
                    server.accept().await
                }
            };
            let client = async {
                match class {
                    PeerClass::NativeBridge => connect(endpoint, &key).await,
                    PeerClass::BrowserParent => connect_browser_parent(endpoint, &key).await,
                }
            };
            let (s, c) = tokio::join!(accept, client);
            let expected = allow_parent || class == PeerClass::NativeBridge;
            let mut refusal = false;
            let observation = match (s, c) {
                (Ok(s), Ok(c)) => {
                    let classes = (s.peer_class(), c.peer_class());
                    let watches = (s.cancellation(), c.cancellation());
                    let (mut reader, server_writer) = s.split();
                    let (client_reader, mut writer) = c.split();
                    // This is opaque, caller-controlled application data, not an
                    // authentication field. Future dispatch must validate it.
                    let claim = br#"{"peer_class":"browser_parent","permit":true}"#;
                    let sent = writer.write(claim).await;
                    let received = if sent.is_ok() {
                        tokio::time::timeout(std::time::Duration::from_secs(5), reader.read())
                            .await
                            .unwrap_or(Err(Error::Deadline))
                    } else {
                        Err(Error::Transport)
                    };
                    drop((reader, writer, server_writer, client_reader));
                    Some((classes, watches, sent, received))
                }
                (s, c) => {
                    refusal = matches!(s, Err(Error::Authentication)) && c.is_err();
                    drop((s, c));
                    None
                }
            };
            let slots = server.permits.available_permits();
            let failed = server.cancellation_failed();
            drop(server);
            drop(Server::bind(endpoint, key).unwrap());
            assert_eq!(observation.is_some(), expected);
            assert_eq!(refusal, !expected);
            assert_eq!(slots, usize::from(MAX_CLIENTS));
            assert!(!failed);
            if let Some((classes, watches, sent, received)) = observation {
                assert_eq!(classes, (class, class));
                assert_eq!(watches.0.status(), CancellationStatus::Requested);
                assert_eq!(watches.1.status(), CancellationStatus::Requested);
                assert_eq!(sent, Ok(()));
                assert_eq!(
                    received.unwrap(),
                    br#"{"peer_class":"browser_parent","permit":true}"#
                );
            }
        }
    }
}
