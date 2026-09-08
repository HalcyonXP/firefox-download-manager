//! Actual helper/stdio inspection in isolated application state, with synthetic secrets only.
use std::fs;
use std::io::Read;
use std::path::PathBuf;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::thread::JoinHandle;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use download_manager_protocol::{read_frame, write_frame};
use download_manager_test_server::{ServerConfig, TestServer};
use serde_json::{Value, json};

struct Host {
    child: Child,
    input: Option<ChildStdin>,
    output: Receiver<Value>,
    reader: Option<JoinHandle<()>>,
    errors: Option<JoinHandle<Vec<u8>>>,
    observed: Vec<Value>,
}
impl Host {
    fn start(root: &Root) -> Self {
        let mut child = Command::new(env!("CARGO_BIN_EXE_download-manager-native-host"))
            .env("LOCALAPPDATA", root.0.join("Local App Data"))
            .env("USERPROFILE", root.0.join("Profile"))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("isolated helper");
        let input = child.stdin.take();
        let mut output = child.stdout.take().expect("stdout");
        let errors = child.stderr.take().expect("stderr");
        let (tx, rx) = mpsc::channel();
        let reader = std::thread::spawn(move || {
            while let Some(body) = read_frame(&mut output).expect("bounded helper frame") {
                if tx
                    .send(serde_json::from_slice(&body).expect("JSON"))
                    .is_err()
                {
                    break;
                }
            }
        });
        let errors = std::thread::spawn(move || {
            let mut bytes = Vec::new();
            errors
                .take(64 * 1024 + 1)
                .read_to_end(&mut bytes)
                .expect("stderr");
            bytes
        });
        Self {
            child,
            input,
            output: rx,
            reader: Some(reader),
            errors: Some(errors),
            observed: Vec::new(),
        }
    }
    fn send(&mut self, correlation: &str, command: &str, payload: &Value) {
        write_frame(self.input.as_mut().expect("input"), &json!({"protocol_version":2,"correlation_id":correlation,"kind":"command","command":command,"payload":payload})).expect("command");
    }
    fn wait(&mut self, predicate: impl Fn(&Value) -> bool) -> Value {
        let deadline = Instant::now() + Duration::from_secs(15);
        loop {
            let value = self
                .output
                .recv_timeout(deadline.saturating_duration_since(Instant::now()))
                .expect("response deadline");
            self.observed.push(value.clone());
            assert!(self.observed.len() < 512, "fixture output is bounded");
            if predicate(&value) {
                return value;
            }
        }
    }
    fn finish(&mut self) {
        drop(self.input.take());
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            if let Some(status) = self.child.try_wait().expect("exit status") {
                assert!(status.success());
                break;
            }
            assert!(Instant::now() < deadline, "helper EOF shutdown deadline");
            std::thread::sleep(Duration::from_millis(10));
        }
        self.reader
            .take()
            .expect("reader")
            .join()
            .expect("reader exit");
        self.observed.extend(self.output.try_iter());
        assert!(self.observed.len() < 512);
        let stderr = self
            .errors
            .take()
            .expect("stderr reader")
            .join()
            .expect("stderr exit");
        assert!(stderr.is_empty(), "clean shutdown has no stderr");
    }
}
impl Drop for Host {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
struct Root(PathBuf);
impl Root {
    fn new() -> Self {
        let root = Self(std::env::temp_dir().join(format!(
                "dm privacy inspection {}-{}",
                std::process::id(),
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .expect("clock")
                    .as_nanos()
            )));
        fs::create_dir_all(root.0.join("Profile/Downloads")).expect("destination");
        fs::create_dir_all(root.0.join("Local App Data")).expect("application root");
        root
    }
    fn state(&self) -> PathBuf {
        self.0
            .join("Local App Data/HalcyonXP/FirefoxDownloadManager/state")
    }
}
impl Drop for Root {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn real_helper_verbose_logs_and_state_exclude_context_secrets_and_expose_only_intended_url_recovery()
 {
    let root = Root::new();
    let server = TestServer::start(ServerConfig::default()).expect("fixture");
    let mut host = Host::start(&root);
    host.send(
        "privacy-hello",
        "hello",
        &json!({"supported_versions":[2],"client_name":"privacy-test","client_version":"0.1.0"}),
    );
    let hello = host.wait(|value| value["command"] == "hello");
    assert!(
        hello["result"]["capabilities"]
            .as_array()
            .expect("capabilities")
            .iter()
            .any(|value| value == "sha256")
    );
    host.wait(|value| value["event"] == "snapshot");
    host.send(
        "privacy-settings",
        "update_settings",
        &json!({"settings":{"verbose_logging":true}}),
    );
    assert_eq!(
        host.wait(|value| value["correlation_id"] == "privacy-settings")["ok"],
        true
    );
    host.send("privacy-denied","add",&json!({"url":server.url("/session/fixture"),"request_context":{"credentials":{"authorization":{"scheme":"Bearer","value":"synthetic-auth-canary26"}}}}));
    assert_eq!(
        host.wait(|value| value["correlation_id"] == "privacy-denied")["ok"],
        false
    );
    host.send("privacy-add","add",&json!({
        "url":server.url("/session/fixture?query_canary=synthetic-query26"),"suggested_filename":"privacy.bin",
        "checksum":{"algorithm":"sha256","digest":"f".repeat(64)},
        "request_context":{"referrer":server.url("/session/page"),"credentials":{"cookies":[{
            "name":"fixture_session","value":"not-a-real-session","domain":"127.0.0.1","path":"/session","secure":false,"http_only":true,"expires_at":null
        }]}}
    }));
    let added = host.wait(|value| value["correlation_id"] == "privacy-add");
    assert_eq!(added["ok"], true);
    assert_eq!(
        host.wait(|value| value["event"] == "failed")["data"]["error"]["code"],
        "CHECKSUM_MISMATCH"
    );
    host.finish();
    let output = serde_json::to_string(&host.observed).expect("output");
    for marker in [
        "not-a-real-session",
        "synthetic-auth-canary26",
        "/session/page",
        "synthetic-query26",
    ] {
        assert!(!output.contains(marker), "native output disclosed input");
    }
    assert!(!root.0.join("Profile/Downloads/privacy.bin").exists());
    assert!(!server.requests().is_empty());
    assert!(
        server
            .requests()
            .iter()
            .all(|request| request.session.fixture_valid)
    );
    inspect_state(&root, added["result"]["task_id"].as_str().expect("task ID"));
}

fn inspect_state(root: &Root, task_id: &str) {
    let state = root.state();
    let record = fs::read_to_string(state.join("tasks").join(format!("{task_id}.task.json")))
        .expect("task record");
    assert!(
        record.contains("synthetic-query26"),
        "exact signed query is intentionally retained for recovery"
    );
    for marker in [
        "not-a-real-session",
        "synthetic-auth-canary26",
        "/session/page",
    ] {
        assert!(!record.contains(marker), "state contains context secret");
    }
    let value: Value = serde_json::from_str(&record).expect("record");
    assert_eq!(value["version"], 4);
    assert_eq!(value["task"]["needs_session"], true);
    assert_eq!(value["task"]["expected_sha256"], "f".repeat(64));
    let settings = fs::read_to_string(state.join("settings.json")).expect("settings");
    assert!(!settings.contains("synthetic-"));
    let log = fs::read_to_string(state.join("diagnostics.log")).expect("diagnostics");
    assert!(log.contains("command_received"));
    assert!(log.contains("task_failed"));
    assert!(log.len() <= 64 * 1024);
    for line in log.lines() {
        let (time, event) = line.split_once(' ').expect("fixed record");
        assert!(time.bytes().all(|byte| byte.is_ascii_digit()));
        assert!(
            [
                "host_started",
                "settings_applied",
                "command_received",
                "task_failed"
            ]
            .contains(&event)
        );
    }
    assert!(
        !log.contains("synthetic-") && !log.contains("not-a-real-session") && !log.contains("http")
    );
}
