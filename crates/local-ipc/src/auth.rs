use std::{fmt, time::Duration};

use hmac::{Hmac, KeyInit, Mac};
use sha2::Sha256;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use uuid::{Uuid, Variant, Version};

use crate::{Channel, Error};

pub(crate) const HANDSHAKE_LIMIT: Duration = Duration::from_secs(2);
const MAGIC: &[u8; 8] = b"DMIPC\x01\0\0";
const CLIENT: &[u8] = b"DMIPC1/client-proof";
const SERVER: &[u8] = b"DMIPC1/server-proof";
const PARENT_CLIENT: &[u8] = b"DMIPC1/browser-parent/client-proof";
const PARENT_SERVER: &[u8] = b"DMIPC1/browser-parent/server-proof";

/// Authenticated proof domain, not add-on identity or browser policy authority.
/// A capability holder can select either class. The native application must bind
/// selection to a fixed entry mode before forwarding any untrusted stdio input.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PeerClass {
    /// Unchanged ordinary bridge proof labels.
    NativeBridge,
    /// Explicit opt-in proof labels, never inferred from application messages.
    BrowserParent,
}

impl PeerClass {
    const fn client_label(self) -> &'static [u8] {
        match self {
            Self::NativeBridge => CLIENT,
            Self::BrowserParent => PARENT_CLIENT,
        }
    }

    const fn server_label(self) -> &'static [u8] {
        match self {
            Self::NativeBridge => SERVER,
            Self::BrowserParent => PARENT_SERVER,
        }
    }
}

/// Transport key. Explicit protected runtime storage requires a separate authority gate.
/// No serialization, automatic logging or secure-erasure claim is provided.
pub struct Capability([u8; 32]);

impl Capability {
    /// Generate a key using the OS randomness source, failing closed.
    /// # Errors
    /// Returns `Randomness` if the OS source fails.
    pub fn generate() -> Result<Self, Error> {
        Ok(Self(random()?))
    }

    /// Borrow secret bytes only for independently verified private runtime storage.
    /// Never log, put in process arguments or confuse with HTTP session credentials.
    #[must_use]
    pub const fn bytes_for_private_storage(&self) -> &[u8; 32] {
        &self.0
    }

    /// Import exactly 32 bytes from independently verified private authority.
    /// This does not establish that authority, freshness or entropy.
    #[must_use]
    pub const fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }
}

impl fmt::Debug for Capability {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Capability([redacted])")
    }
}

/// A local, canonical `UUIDv4` address. Never an arbitrary path or remote hostname.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Endpoint(Uuid);

impl Endpoint {
    /// Generate a fresh endpoint; this is not an installed per-user singleton.
    /// # Errors
    /// Returns `Randomness` if the OS source fails.
    pub fn generate() -> Result<Self, Error> {
        Ok(Self(
            uuid::Builder::from_random_bytes(random()?).into_uuid(),
        ))
    }

    /// Parse an address from independently verified authority, with no aliases.
    /// # Errors
    /// Rejects noncanonical/non-v4 IDs and all raw path/hostname inputs.
    pub fn parse(value: &str) -> Result<Self, Error> {
        if value.len() != 36 {
            return Err(Error::Endpoint);
        }
        let id = Uuid::parse_str(value).map_err(|_| Error::Endpoint)?;
        if id.get_version() != Some(Version::Random)
            || id.get_variant() != Variant::RFC4122
            || id.hyphenated().to_string() != value
        {
            return Err(Error::Endpoint);
        }
        Ok(Self(id))
    }

    /// Public address, not a credential. Never use it alone as authority.
    #[must_use]
    pub fn id(&self) -> String {
        self.0.hyphenated().to_string()
    }

    #[cfg(windows)]
    pub(crate) fn path(self) -> String {
        format!(r"\\.\pipe\HalcyonXP.FirefoxDownloadManager.ipc1.{}", self.0)
    }
}

impl fmt::Debug for Endpoint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Endpoint([redacted])")
    }
}

fn random<const N: usize>() -> Result<[u8; N], Error> {
    let mut bytes = [0; N];
    getrandom::fill(&mut bytes).map_err(|_| Error::Randomness)?;
    Ok(bytes)
}

fn proof(
    key: &Capability,
    role: &[u8],
    endpoint: Endpoint,
    server: &[u8; 32],
    client: &[u8; 32],
) -> Hmac<Sha256> {
    // HMAC accepts every key length; this fixed 32-byte input cannot be invalid.
    let mut mac = Hmac::<Sha256>::new_from_slice(&key.0).expect("fixed HMAC key length");
    mac.update(role);
    mac.update(MAGIC);
    mac.update(endpoint.0.as_bytes());
    mac.update(server);
    mac.update(client);
    mac
}

pub(crate) async fn server<S>(
    io: S,
    key: &Capability,
    endpoint: Endpoint,
) -> Result<Channel<S>, Error>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    server_inner(io, key, endpoint, false).await
}

pub(crate) async fn server_inner<S>(
    mut io: S,
    key: &Capability,
    endpoint: Endpoint,
    allow_parent: bool,
) -> Result<Channel<S>, Error>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let class = tokio::time::timeout(HANDSHAKE_LIMIT, async {
        let challenge = random()?;
        io.write_all(MAGIC).await.map_err(|_| Error::Transport)?;
        io.write_all(&challenge)
            .await
            .map_err(|_| Error::Transport)?;
        let mut response = [0; 64];
        io.read_exact(&mut response)
            .await
            .map_err(|_| Error::Transport)?;
        let client: &[u8; 32] = response[..32].try_into().expect("fixed nonce field");
        let class = if proof(key, CLIENT, endpoint, &challenge, client)
            .verify_slice(&response[32..])
            .is_ok()
        {
            PeerClass::NativeBridge
        } else if allow_parent
            && proof(key, PARENT_CLIENT, endpoint, &challenge, client)
                .verify_slice(&response[32..])
                .is_ok()
        {
            PeerClass::BrowserParent
        } else {
            return Err(Error::Authentication);
        };
        let answer = proof(key, class.server_label(), endpoint, &challenge, client)
            .finalize()
            .into_bytes();
        io.write_all(&answer).await.map_err(|_| Error::Transport)?;
        Ok(class)
    })
    .await
    .map_err(|_| Error::Deadline)??;
    Ok(Channel::authenticated(io, class))
}

pub(crate) async fn client<S>(
    io: S,
    key: &Capability,
    endpoint: Endpoint,
) -> Result<Channel<S>, Error>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    client_inner(io, key, endpoint, PeerClass::NativeBridge).await
}

pub(crate) async fn client_inner<S>(
    mut io: S,
    key: &Capability,
    endpoint: Endpoint,
    class: PeerClass,
) -> Result<Channel<S>, Error>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    tokio::time::timeout(HANDSHAKE_LIMIT, async {
        let mut hello = [0; 40];
        io.read_exact(&mut hello)
            .await
            .map_err(|_| Error::Transport)?;
        if &hello[..8] != MAGIC {
            return Err(Error::Authentication);
        }
        let server: &[u8; 32] = hello[8..].try_into().expect("fixed nonce field");
        let nonce = random()?;
        let answer = proof(key, class.client_label(), endpoint, server, &nonce)
            .finalize()
            .into_bytes();
        io.write_all(&nonce).await.map_err(|_| Error::Transport)?;
        io.write_all(&answer).await.map_err(|_| Error::Transport)?;
        let mut confirmation = [0; 32];
        io.read_exact(&mut confirmation)
            .await
            .map_err(|_| Error::Transport)?;
        proof(key, class.server_label(), endpoint, server, &nonce)
            .verify_slice(&confirmation)
            .map_err(|_| Error::Authentication)
    })
    .await
    .map_err(|_| Error::Deadline)??;
    Ok(Channel::authenticated(io, class))
}

#[cfg(test)]
#[path = "auth_parent_tests.rs"]
mod parent_tests;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn endpoint_and_debug_do_not_accept_or_disclose_untrusted_addresses() {
        let endpoint = Endpoint::generate().unwrap();
        assert_eq!(Endpoint::parse(&endpoint.id()).unwrap(), endpoint);
        for bad in [
            "",
            "\\\\remote\\pipe\\x",
            "00000000-0000-0000-0000-000000000000",
            "AAAAAAAA-AAAA-4AAA-8AAA-AAAAAAAAAAAA",
            "aaaaaaaaaaaa4aaa8aaaaaaaaaaaaaaa",
        ] {
            assert_eq!(Endpoint::parse(bad), Err(Error::Endpoint));
        }
        assert_eq!(
            format!("{:?}", Capability::from_bytes([67; 32])),
            "Capability([redacted])"
        );
        assert_eq!(format!("{endpoint:?}"), "Endpoint([redacted])");
    }

    #[test]
    fn transcript_matches_independently_generated_hmac_sha256_vector() {
        // Python stdlib hmac/hashlib, explicit protocol fields, no live material.
        let expected = [
            229, 223, 51, 124, 10, 244, 183, 215, 170, 152, 168, 19, 149, 92, 254, 204, 140, 70,
            145, 246, 144, 11, 41, 160, 235, 225, 190, 238, 238, 219, 4, 80,
        ];
        let endpoint = Endpoint::parse("11111111-1111-4111-8111-111111111111").unwrap();
        let key = Capability::from_bytes([11; 32]);
        assert!(
            proof(&key, CLIENT, endpoint, &[1; 32], &[2; 32])
                .verify_slice(&expected)
                .is_ok()
        );
    }

    #[test]
    fn proofs_bind_role_endpoint_and_both_fresh_challenges() {
        let key = Capability::from_bytes([11; 32]);
        let endpoint = Endpoint::generate().unwrap();
        let tag = proof(&key, CLIENT, endpoint, &[1; 32], &[2; 32])
            .finalize()
            .into_bytes();
        for mac in [
            proof(&key, SERVER, endpoint, &[1; 32], &[2; 32]),
            proof(
                &key,
                CLIENT,
                Endpoint::generate().unwrap(),
                &[1; 32],
                &[2; 32],
            ),
            proof(&key, CLIENT, endpoint, &[3; 32], &[2; 32]),
            proof(&key, CLIENT, endpoint, &[1; 32], &[3; 32]),
            proof(
                &Capability::from_bytes([12; 32]),
                CLIENT,
                endpoint,
                &[1; 32],
                &[2; 32],
            ),
        ] {
            assert!(mac.verify_slice(&tag).is_err());
        }
        assert!(
            proof(&key, CLIENT, endpoint, &[1; 32], &[2; 32])
                .verify_slice(&tag)
                .is_ok()
        );
    }

    #[tokio::test]
    async fn mutual_authentication_and_wrong_key_refusal() {
        let endpoint = Endpoint::generate().unwrap();
        let key = Capability::generate().unwrap();
        let wrong = Capability::generate().unwrap();
        for (client_key, accepted) in [(&key, true), (&wrong, false)] {
            let (a, b) = tokio::io::duplex(256);
            let (s, c) = tokio::join!(server(a, &key, endpoint), client(b, client_key, endpoint));
            if accepted {
                assert!(s.is_ok() && c.is_ok());
            } else {
                assert!(matches!(s, Err(Error::Authentication)));
                assert!(c.is_err());
            }
        }
    }

    #[tokio::test]
    async fn unauthenticated_server_receives_no_application_data() {
        let endpoint = Endpoint::generate().unwrap();
        let key = Capability::generate().unwrap();
        let (mut hostile, victim) = tokio::io::duplex(256);
        let attacker = async {
            hostile.write_all(MAGIC).await.unwrap();
            hostile.write_all(&[9; 32]).await.unwrap();
            let mut response = [0; 64];
            hostile.read_exact(&mut response).await.unwrap();
            hostile.write_all(&[0; 32]).await.unwrap();
            let mut extra = [0; 1];
            assert_eq!(hostile.read(&mut extra).await.unwrap(), 0);
        };
        let ((), result) = tokio::join!(attacker, client(victim, &key, endpoint));
        assert!(matches!(result, Err(Error::Authentication)));
    }

    #[tokio::test]
    async fn silent_peer_hits_whole_handshake_deadline() {
        let (_silent, victim) = tokio::io::duplex(256);
        assert!(matches!(
            client(
                victim,
                &Capability::generate().unwrap(),
                Endpoint::generate().unwrap()
            )
            .await,
            Err(Error::Deadline)
        ));
    }
}
