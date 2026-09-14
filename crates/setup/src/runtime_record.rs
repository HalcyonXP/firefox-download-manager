//! Protected per-session discovery. A candidate/record is not a live engine receipt.
use std::{fs, sync::Arc};

use download_manager_local_ipc::{Capability, Endpoint, Server};
use serde::{Deserialize, Serialize};

use crate::{
    SetupError,
    installed_image::{ImageBinding, InstalledImage},
    paths::DirectoryLease,
    private_directory::{PrivateDirectory, session_name},
    private_file::{LIMIT, PrivateFile, PublishedFile},
};

const ERROR: SetupError = SetupError::Ownership;
const MAX_ROOT_ENTRIES: usize = 64;
const PREFIX: &str = "companion-runtime.";
const HEX: &[u8; 16] = b"0123456789abcdef";

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Stored {
    format: String,
    version: u8,
    ipc_version: u8,
    protocol_version: u8,
    binding: ImageBinding,
    endpoint: String,
    capability_hex: String,
}

/// Keeps the verified image, bound listener and successful creation witnesses live.
/// Caller must independently hold the engine state lock before publication.
pub struct RuntimePublication<'a> {
    _image: &'a InstalledImage,
    server: Server,
    directory: PrivateDirectory,
    record: PublishedFile,
}
impl<'a> RuntimePublication<'a> {
    /// Bind a fresh listener, publish its protected record, and only then expose
    /// the listener. No caller can serve from a failed/unfinished publisher.
    /// Caller must hold engine ownership and run under an active Tokio runtime.
    /// # Errors
    /// Failed/existing directory or record creation preserves the domain and
    /// retires the unexposed listener. This is not authority to adopt a stale record.
    pub fn bind(
        image: &'a InstalledImage,
        endpoint: Endpoint,
        capability: Arc<Capability>,
    ) -> Result<Self, SetupError> {
        let server = Server::bind(endpoint, capability).map_err(|_| ERROR)?;
        let bytes = encode(
            image,
            server.endpoint(),
            server.capability_for_private_storage(),
        )?;
        let directory =
            PrivateDirectory::create_for_endpoint(image.root_lease()?, server.endpoint())?;
        let record = PublishedFile::create(directory.lease()?, &bytes)?;
        Ok(Self {
            _image: image,
            server,
            directory,
            record,
        })
    }

    /// The listener becomes accessible only after complete successful publication.
    #[must_use]
    pub const fn server(&self) -> &Server {
        &self.server
    }

    /// After joined session/engine retirement, remove only this creator's record
    /// and empty session directory. Failed/unknown domains are never swept.
    /// # Errors
    /// Competing readers, unexpected children or failed cleanup refuse success.
    pub fn remove(self) -> Result<(), SetupError> {
        if self.server.cancellation_failed() {
            return Err(ERROR);
        }
        self.record.remove()?;
        self.directory.remove()
    }
}

/// A private, installation-matching candidate; authentication/liveness is separate.
/// No Debug/Serialize; retain through authentication, then discard the key/record.
pub struct RuntimeConnection<'a> {
    _image: &'a InstalledImage,
    _record: PrivateFile,
    endpoint: Endpoint,
    capability: Capability,
}
impl<'a> RuntimeConnection<'a> {
    /// Read only the endpoint-derived leaf under the verified install root.
    /// File permissions are verified before secret bytes are read/decoded.
    /// # Errors
    /// Refuses malformed/mismatching/incomplete records without modifying them.
    pub fn open(image: &'a InstalledImage, endpoint: Endpoint) -> Result<Self, SetupError> {
        let root = image.root_lease()?;
        let directory = DirectoryLease::open(&root.path().join(session_name(endpoint)))?;
        let record = PrivateFile::open(directory)?;
        let capability = decode(record.bytes(), image, endpoint)?;
        Ok(Self {
            _image: image,
            _record: record,
            endpoint,
            capability,
        })
    }

    /// Candidate address, never proof that its peer owns an engine.
    #[must_use]
    pub const fn endpoint(&self) -> Endpoint {
        self.endpoint
    }

    /// Borrow only for authenticated local connection; do not log or replay Add.
    #[must_use]
    pub const fn capability(&self) -> &Capability {
        &self.capability
    }
}

/// At most 64 total root entries are inspected. Unknown non-session entries are
/// preserved. Results are untrusted hints, not authority or a cleanup list.
/// No connection retry, process launch or secret read is performed here.
/// # Errors
/// Too many entries, malformed session names and enumeration errors fail closed.
pub fn candidates(image: &InstalledImage) -> Result<Vec<Endpoint>, SetupError> {
    let root = image.root_lease()?;
    let mut found = Vec::new();
    for (index, entry) in fs::read_dir(root.path()).map_err(|_| ERROR)?.enumerate() {
        if index >= MAX_ROOT_ENTRIES {
            return Err(ERROR);
        }
        let name = entry.map_err(|_| ERROR)?.file_name();
        let Some(name) = name.to_str() else {
            continue;
        };
        if let Some(id) = name.strip_prefix(PREFIX) {
            found.push(Endpoint::parse(id).map_err(|_| ERROR)?);
        }
    }
    Ok(found)
}

fn encode(
    image: &InstalledImage,
    endpoint: Endpoint,
    capability: &Capability,
) -> Result<Vec<u8>, SetupError> {
    let capability_hex = capability
        .bytes_for_private_storage()
        .iter()
        .flat_map(|byte| {
            [
                char::from(HEX[usize::from(byte >> 4)]),
                char::from(HEX[usize::from(byte & 15)]),
            ]
        })
        .collect();
    let stored = Stored {
        format: "firefox-download-manager-runtime".into(),
        version: 1,
        ipc_version: 1,
        protocol_version: 2,
        binding: image.binding.clone(),
        endpoint: endpoint.id(),
        capability_hex,
    };
    let bytes = serde_json::to_vec(&stored).map_err(|_| ERROR)?;
    if bytes.len() > LIMIT {
        return Err(ERROR);
    }
    Ok(bytes)
}

fn decode(
    bytes: &[u8],
    image: &InstalledImage,
    endpoint: Endpoint,
) -> Result<Capability, SetupError> {
    if bytes.is_empty() || bytes.len() > LIMIT {
        return Err(ERROR);
    }
    let stored: Stored = serde_json::from_slice(bytes).map_err(|_| ERROR)?;
    if stored.format != "firefox-download-manager-runtime"
        || stored.version != 1
        || stored.ipc_version != 1
        || stored.protocol_version != 2
        || stored.binding != image.binding
        || stored.endpoint != endpoint.id()
        || stored.capability_hex.len() != 64
    {
        return Err(ERROR);
    }
    let mut key = [0_u8; 32];
    for (target, pair) in key
        .iter_mut()
        .zip(stored.capability_hex.as_bytes().chunks_exact(2))
    {
        *target = (digit(pair[0])? << 4) | digit(pair[1])?;
    }
    Ok(Capability::from_bytes(key))
}
fn digit(byte: u8) -> Result<u8, SetupError> {
    match byte {
        b'0'..=b'9' => Ok(byte - b'0'),
        b'a'..=b'f' => Ok(byte - b'a' + 10),
        _ => Err(ERROR),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::installed_image::tests::Fixture;

    #[test]
    fn closed_bounded_record_matches_independent_image_and_endpoint() {
        let fixture = Fixture::new();
        let image = fixture.inspect().unwrap();
        let endpoint = Endpoint::generate().unwrap();
        let key = Capability::from_bytes([0x5a; 32]); // Public deterministic fixture, not a secret.
        let bytes = encode(&image, endpoint, &key).unwrap();
        assert_eq!(
            decode(&bytes, &image, endpoint)
                .unwrap()
                .bytes_for_private_storage(),
            &[0x5a; 32]
        );
        assert!(decode(&bytes, &image, Endpoint::generate().unwrap()).is_err());
        let mut padded = bytes.clone();
        padded.resize(LIMIT, b' ');
        assert!(decode(&padded, &image, endpoint).is_ok());
        padded.push(b' ');
        assert!(decode(&padded, &image, endpoint).is_err());
        let other = Fixture::new();
        let other_image = other.inspect().unwrap();
        assert!(decode(&bytes, &other_image, endpoint).is_err());
        for (field, value) in [
            ("format", serde_json::json!("other")),
            ("version", serde_json::json!(2)),
            ("ipc_version", serde_json::json!(2)),
            ("protocol_version", serde_json::json!(1)),
            ("unknown", serde_json::json!(true)),
            ("capability_hex", serde_json::json!("5A".repeat(32))),
            ("capability_hex", serde_json::json!("5a".repeat(31))),
        ] {
            let mut value_json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
            value_json[field] = value;
            assert!(decode(&serde_json::to_vec(&value_json).unwrap(), &image, endpoint).is_err());
        }
        let mut nested: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        nested["binding"]["unknown"] = true.into();
        assert!(decode(&serde_json::to_vec(&nested).unwrap(), &image, endpoint).is_err());
        let mut duplicate = bytes.clone();
        duplicate.pop();
        duplicate.extend_from_slice(b",\"version\":1}");
        assert!(decode(&duplicate, &image, endpoint).is_err());
        drop(other_image);
        other.remove();
        drop(image);
        fixture.remove();
    }

    #[test]
    fn published_private_record_authenticates_actual_pipe_without_adopting_entries() {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .unwrap();
        let enter = runtime.enter();
        let fixture = Fixture::new();
        let image = fixture.inspect().unwrap();
        let endpoint = Endpoint::generate().unwrap();
        let publication = RuntimePublication::bind(
            &image,
            endpoint,
            Arc::new(Capability::from_bytes([0x5a; 32])),
        )
        .unwrap();
        assert!(
            RuntimePublication::bind(
                &image,
                endpoint,
                Arc::new(Capability::from_bytes([0x5a; 32]))
            )
            .is_err()
        );
        assert_eq!(candidates(&image).unwrap(), vec![endpoint]);
        let connection = RuntimeConnection::open(&image, endpoint).unwrap();
        runtime.block_on(async {
            let (peer, client) = tokio::join!(
                publication.server().accept(),
                download_manager_local_ipc::connect(connection.endpoint(), connection.capability())
            );
            drop(peer.unwrap());
            drop(client.unwrap());
        });
        drop(connection);
        assert!(!publication.server().cancellation_failed());
        publication.remove().unwrap();
        assert!(candidates(&image).unwrap().is_empty());
        let other_endpoint = Endpoint::generate().unwrap();
        let other_publication = RuntimePublication::bind(
            &image,
            other_endpoint,
            Arc::new(Capability::from_bytes([0x5a; 32])),
        )
        .unwrap();
        let reader = RuntimeConnection::open(&image, other_endpoint).unwrap();
        assert!(other_publication.remove().is_err());
        drop(reader);
        assert_eq!(candidates(&image).unwrap(), vec![other_endpoint]);
        // Failed removal is preserved. A retry can bind the absent pipe, but
        // must refuse the existing record and retire its unexposed listener.
        assert!(
            RuntimePublication::bind(
                &image,
                other_endpoint,
                Arc::new(Capability::from_bytes([0x5a; 32]))
            )
            .is_err()
        );
        runtime.block_on(async {
            assert!(
                matches!(
                    download_manager_local_ipc::connect(
                        other_endpoint,
                        &Capability::from_bytes([0x5a; 32])
                    )
                    .await,
                    Err(download_manager_local_ipc::Error::Transport)
                ),
                "failed publication must close the actual listener, not just fail authentication"
            );
        });
        drop(enter);
        drop(runtime); // Workers join before owned fixture cleanup/success.
        drop(image);
        fixture.remove();
    }

    #[test]
    fn enumeration_refuses_malformed_or_excess_entries_and_preserves_unknown_files() {
        let fixture = Fixture::new();
        let image = fixture.inspect().unwrap();
        let root = image.root_lease().unwrap();
        let bad = root.path().join("companion-runtime.not-a-uuid");
        fs::write(&bad, b"public unknown entry").unwrap();
        assert!(candidates(&image).is_err());
        assert_eq!(fs::read(&bad).unwrap(), b"public unknown entry");
        fs::remove_file(bad).unwrap();
        for index in 0..MAX_ROOT_ENTRIES {
            fs::write(root.path().join(format!("public-{index}")), b"").unwrap();
        }
        assert!(candidates(&image).is_err());
        drop(root);
        drop(image);
        fixture.remove();
    }
}
