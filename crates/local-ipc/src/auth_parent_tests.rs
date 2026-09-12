use super::*;

#[test]
fn parent_proof_domains_match_independent_vectors_without_changing_ordinary_labels() {
    // Independently generated with Python stdlib hmac/hashlib and explicit fields.
    let client = [
        52, 26, 172, 23, 7, 250, 79, 43, 198, 180, 238, 118, 214, 188, 114, 139, 5, 215, 239, 130,
        228, 255, 245, 148, 191, 217, 216, 141, 33, 245, 162, 218,
    ];
    let server = [
        44, 147, 171, 36, 81, 118, 4, 153, 160, 84, 123, 45, 216, 76, 211, 123, 69, 151, 168, 10,
        80, 223, 168, 14, 113, 95, 237, 187, 75, 160, 197, 98,
    ];
    let endpoint = Endpoint::parse("11111111-1111-4111-8111-111111111111").unwrap();
    let key = Capability::from_bytes([11; 32]);
    for (label, expected) in [(PARENT_CLIENT, client), (PARENT_SERVER, server)] {
        assert!(
            proof(&key, label, endpoint, &[1; 32], &[2; 32])
                .verify_slice(&expected)
                .is_ok()
        );
        for other in [CLIENT, SERVER, PARENT_CLIENT, PARENT_SERVER] {
            if other != label {
                assert!(
                    proof(&key, other, endpoint, &[1; 32], &[2; 32])
                        .verify_slice(&expected)
                        .is_err()
                );
            }
        }
    }
}

#[tokio::test]
async fn parent_class_requires_explicit_acceptance_and_matching_mutual_proofs() {
    let endpoint = Endpoint::generate().unwrap();
    let key = Capability::generate().unwrap();
    let wrong = Capability::generate().unwrap();
    for allow_parent in [false, true] {
        for class in [PeerClass::NativeBridge, PeerClass::BrowserParent] {
            for correct_key in [false, true] {
                let (a, b) = tokio::io::duplex(256);
                let accept = async {
                    if allow_parent {
                        server_inner(a, &key, endpoint, true).await
                    } else {
                        server(a, &key, endpoint).await
                    }
                };
                let client_key = if correct_key { &key } else { &wrong };
                let connect = async {
                    match class {
                        PeerClass::NativeBridge => client(b, client_key, endpoint).await,
                        PeerClass::BrowserParent => {
                            client_inner(b, client_key, endpoint, class).await
                        }
                    }
                };
                let (s, c) = tokio::join!(accept, connect);
                // Consume/drop both owners before checking the completed observations.
                let s = s.map(|channel| channel.peer_class());
                let c = c.map(|channel| channel.peer_class());
                if correct_key && (allow_parent || class == PeerClass::NativeBridge) {
                    assert_eq!(s, Ok(class));
                    assert_eq!(c, Ok(class));
                } else {
                    assert_eq!(s, Err(Error::Authentication));
                    assert!(c.is_err());
                }
            }
        }
    }
}

#[tokio::test]
async fn server_confirmation_cannot_change_the_requested_peer_class() {
    let endpoint = Endpoint::generate().unwrap();
    let key = Capability::generate().unwrap();
    for class in [PeerClass::NativeBridge, PeerClass::BrowserParent] {
        let (mut hostile, victim) = tokio::io::duplex(256);
        let attacker = async {
            hostile.write_all(MAGIC).await.unwrap();
            hostile.write_all(&[9; 32]).await.unwrap();
            let mut response = [0; 64];
            hostile.read_exact(&mut response).await.unwrap();
            let nonce = response[..32].try_into().unwrap();
            proof(&key, class.client_label(), endpoint, &[9; 32], nonce)
                .verify_slice(&response[32..])
                .unwrap();
            let other = match class {
                PeerClass::NativeBridge => PARENT_SERVER,
                PeerClass::BrowserParent => SERVER,
            };
            let tag = proof(&key, other, endpoint, &[9; 32], nonce)
                .finalize()
                .into_bytes();
            hostile.write_all(&tag).await.unwrap();
            let mut byte = [0];
            hostile.read(&mut byte).await.unwrap()
        };
        let client = async {
            // Even an erroneous accepted channel must close before the peer's
            // EOF observation; retaining it in join!'s result would deadlock.
            client_inner(victim, &key, endpoint, class)
                .await
                .map(|channel| channel.peer_class())
        };
        let (extra, result) = tokio::join!(attacker, client);
        assert_eq!(extra, 0);
        assert!(matches!(result, Err(Error::Authentication)));
    }
}

#[tokio::test]
async fn unknown_or_reflected_client_proofs_never_select_a_class() {
    let endpoint = Endpoint::generate().unwrap();
    let key = Capability::generate().unwrap();
    for label in [SERVER, PARENT_SERVER, b"DMIPC1/unknown".as_slice()] {
        let (server_io, mut hostile) = tokio::io::duplex(256);
        let attacker = async {
            let mut hello = [0; 40];
            hostile.read_exact(&mut hello).await.unwrap();
            let challenge = hello[8..].try_into().unwrap();
            let tag = proof(&key, label, endpoint, challenge, &[7; 32])
                .finalize()
                .into_bytes();
            hostile.write_all(&[7; 32]).await.unwrap();
            hostile.write_all(&tag).await.unwrap();
            let mut byte = [0];
            hostile.read(&mut byte).await.unwrap()
        };
        let accept = async {
            server_inner(server_io, &key, endpoint, true)
                .await
                .map(|channel| channel.peer_class())
        };
        let (result, extra) = tokio::join!(accept, attacker);
        assert_eq!(extra, 0);
        assert!(matches!(result, Err(Error::Authentication)));
    }
}

#[tokio::test]
async fn parent_client_silent_peer_uses_the_existing_whole_handshake_deadline() {
    let (_silent, victim) = tokio::io::duplex(256);
    assert!(matches!(
        client_inner(
            victim,
            &Capability::generate().unwrap(),
            Endpoint::generate().unwrap(),
            PeerClass::BrowserParent,
        )
        .await,
        Err(Error::Deadline)
    ));
}
