//! Connection admission, NOT captured-request or publication authority.
use std::io::Write;

use download_manager_engine::persistence::TaskId;
use tokio::sync::mpsc;

use super::{HostError, Inbound, Session};

#[derive(serde::Serialize)]
struct Announcement<'a> {
    parent_transport: u8,
    admission_id: &'a str,
    kind: &'static str,
    capture_ready: bool,
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Acceptance {
    parent_transport: u8,
    admission_id: String,
    kind: String,
}

fn accepts(body: &[u8], id: &str) -> bool {
    // Independent of the larger wire2 frame bound; never log input or this ID.
    if body.len() > 256 {
        return false;
    }
    serde_json::from_slice::<Acceptance>(body).is_ok_and(|ack| {
        ack.parent_transport == 1 && ack.admission_id == id && ack.kind == "accept"
    })
}

pub(super) async fn admit<W: Write>(
    session: &Session<'_, W>,
    inbound: &mut mpsc::Receiver<Inbound>,
) -> Result<(), HostError> {
    // UUIDv4 generation is reused, not a persisted task or protection epoch.
    // Each invocation belongs to one already class-checked retained channel.
    let id = TaskId::new().to_string();
    let mut announcement = Announcement {
        parent_transport: 1,
        admission_id: &id,
        kind: "offer",
        capture_ready: false,
    };
    session.write(&announcement).await?;
    let Some(Inbound::Body(body)) = inbound.recv().await else {
        return Err(HostError::LocalSession);
    };
    if !accepts(&body, &id) {
        return Err(HostError::LocalSession);
    }
    // Offer/write completion is not peer receipt. Only the matching response
    // admits this connection; the caller still must observe this ready frame.
    announcement.kind = "ready";
    session.write(&announcement).await
}

#[cfg(test)]
mod tests {
    use super::accepts;

    #[test]
    fn acceptance_is_closed_typed_bounded_and_exact() {
        let id = "11111111-1111-4111-8111-111111111111";
        let valid = format!(r#"{{"parent_transport":1,"admission_id":"{id}","kind":"accept"}}"#);
        assert!(accepts(valid.as_bytes(), id));
        let invalid = [
            valid.replace(":1,", ":true,"),
            valid.replace(":1,", ":1.0,"),
            valid.replace(":1,", ":2,"),
            valid.replace("accept", "ready"),
            valid.replace("4111", "4112"),
            valid.replace("\"kind\":\"accept\"", "\"kind\":{\"accept\":null}"),
            valid.replace(
                "\"kind\":\"accept\"",
                "\"kind\":\"accept\",\"kind\":\"accept\"",
            ),
            valid.replace(
                "\"kind\":\"accept\"",
                "\"capture_ready\":true,\"kind\":\"accept\"",
            ),
            valid.replace("\"kind\":\"accept\"", "\"kind\":\"accept\",\"wire\":null"),
            format!("{valid} null"),
            format!("{}{valid}", " ".repeat(256)),
        ];
        for body in invalid {
            assert!(!accepts(body.as_bytes(), id));
        }
        assert!(!accepts(&[255], id));
    }
}
