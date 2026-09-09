//! Maintainer-only bounded probe diagnostic; never creates a download task/file.
//! Read one URL from stdin (not arguments), and emit only bounded non-secret fields.
//! This uses the engine library, not the packaged host, companion or Firefox.
use std::io::{self, Read};
use std::process::ExitCode;
use std::time::Duration;

use download_manager_engine::network::{FallbackReason, ProbeClient, ProbeMode, ResourceProbe};
use serde_json::{Value, json};

const INPUT_LIMIT: u64 = 8 * 1024;

fn input_url(reader: impl Read) -> Result<String, &'static str> {
    let mut bytes = Vec::new();
    reader
        .take(INPUT_LIMIT + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "input_read_failed")?;
    if bytes.len() as u64 > INPUT_LIMIT {
        return Err("input_too_large");
    }
    let text = String::from_utf8(bytes).map_err(|_| "input_encoding_invalid")?;
    let url = text.trim();
    if url.is_empty() || url.chars().any(char::is_control) {
        return Err("input_invalid");
    }
    Ok(url.to_owned())
}

fn summary(probe: &ResourceProbe) -> Value {
    let mode = match probe.mode() {
        ProbeMode::Segmented => "segmented",
        ProbeMode::Empty => "empty",
        ProbeMode::SingleStream(FallbackReason::RangeIgnored) => "single_range_ignored",
        ProbeMode::SingleStream(FallbackReason::UnknownLength) => "single_unknown_length",
        ProbeMode::SingleStream(FallbackReason::InsufficientIdentity) => {
            "single_insufficient_identity"
        }
    };
    json!({
        "status": "probe_succeeded",
        "mode": mode,
        "size_bytes": probe.size(),
        "strong_identity": probe.validators().has_strong_identity(),
        "filename_present": probe.filename().is_some(),
        "scope": "engine-library probe only; no download task/file, packaged-host or Firefox proof"
    })
}

async fn observe(url: &str) -> Value {
    let Ok(client) = ProbeClient::new() else {
        return json!({"status": "client_setup_failed"});
    };
    match tokio::time::timeout(Duration::from_secs(60), client.probe(url)).await {
        Ok(Ok(probe)) => summary(&probe),
        // ProbeError's Display is deliberately URL/response-free in the engine.
        // Do not replace it with a debug/source chain, input URL or final URL.
        Ok(Err(error)) => json!({"status": "probe_failed", "reason": error.to_string()}),
        Err(_) => json!({"status": "probe_deadline"}),
    }
}

#[tokio::main]
async fn main() -> ExitCode {
    let result = match input_url(io::stdin().lock()) {
        Ok(url) => observe(&url).await,
        Err(reason) => json!({"status": reason}),
    };
    println!("{result}");
    if result["status"] == "probe_succeeded" {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use download_manager_test_server::{Fixture, ServerConfig, TestServer};

    #[test]
    fn input_is_bounded_and_does_not_echo_rejected_contents() {
        let url = "https://example.test/file?sig=synthetic-private-canary";
        assert_eq!(
            input_url(format!("{url}\r\n").as_bytes()),
            Ok(url.to_owned())
        );
        let mut oversized = io::Cursor::new(vec![b'x'; 16384]);
        assert_eq!(input_url(&mut oversized), Err("input_too_large"));
        assert_eq!(oversized.position(), INPUT_LIMIT + 1);
        assert_eq!(input_url(&vec![b'x'; 8192][..]).unwrap().len(), 8192);
        assert_eq!(input_url(&[0xff][..]), Err("input_encoding_invalid"));
        assert_eq!(input_url(b" \r\n".as_slice()), Err("input_invalid"));
        assert_eq!(
            input_url(b"https://example.test/\nsecret".as_slice()),
            Err("input_invalid")
        );
    }

    #[tokio::test]
    async fn real_fixture_uses_only_boundary_probes_and_redacts_sensitive_metadata() {
        let server = TestServer::start(ServerConfig {
            fixture: Fixture {
                len: 1024,
                seed: 29,
            },
            rules: Vec::new(),
        })
        .expect("owned fixture should bind");
        let result = observe(&server.url("/fixture?sig=synthetic-private-canary")).await;
        assert_eq!(result["status"], "probe_succeeded");
        assert_eq!(result["mode"], "segmented");
        assert_eq!(result["size_bytes"], 1024);
        let rendered = result.to_string();
        assert!(!rendered.contains("synthetic-private-canary"));
        assert!(!rendered.contains("127.0.0.1"));
        let requests = server.requests();
        assert_eq!(requests.len(), 2);
        assert_eq!(requests[0].range.map(|r| (r.start, r.end)), Some((0, 0)));
        assert_eq!(
            requests[1].range.map(|r| (r.start, r.end)),
            Some((1023, 1023))
        );
        let failed = observe("https://synthetic-private-canary@example.test/file").await;
        assert_eq!(failed["status"], "probe_failed");
        assert!(!failed.to_string().contains("synthetic-private-canary"));
    }
}
