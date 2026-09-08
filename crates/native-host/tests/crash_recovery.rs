use std::fs;
use std::path::PathBuf;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use download_manager_protocol::{read_frame, write_frame};
use download_manager_test_server::{
    ByteRange, Fault, FaultRule, Fixture, RequestSelector, ServerConfig, TestServer,
};
use serde_json::{Value, json};

struct Process {
    child: Child,
    input: ChildStdin,
    output: Receiver<Value>,
}
impl Process {
    fn start(root: &std::path::Path) -> Self {
        let mut child = Command::new(env!("CARGO_BIN_EXE_download-manager-native-host"))
            .env("LOCALAPPDATA", root.join("Local App Data"))
            .env("USERPROFILE", root.join("Profile"))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("start helper");
        let input = child.stdin.take().expect("stdin");
        let mut output = child.stdout.take().expect("stdout");
        let (sender, receiver) = mpsc::channel();
        std::thread::spawn(move || {
            while let Ok(Some(body)) = read_frame(&mut output) {
                let value = serde_json::from_slice(&body).expect("valid output");
                if sender.send(value).is_err() {
                    break;
                }
            }
        });
        Self {
            child,
            input,
            output: receiver,
        }
    }
    fn send(&mut self, name: &str, payload: &Value) {
        write_frame(&mut self.input, &json!({"protocol_version":2,"correlation_id":name,"kind":"command","command":name,"payload":payload})).expect("send command");
    }
    fn wait(&self, predicate: impl Fn(&Value) -> bool) -> Value {
        let deadline = Instant::now() + Duration::from_secs(15);
        loop {
            let value = self
                .output
                .recv_timeout(deadline.saturating_duration_since(Instant::now()))
                .expect("helper output deadline");
            if predicate(&value) {
                return value;
            }
        }
    }
    fn hello(&mut self) -> Value {
        self.send(
            "hello",
            &json!({"supported_versions":[2],"client_name":"crash-test","client_version":"0.1.0"}),
        );
        self.wait(|value| value["event"] == "snapshot")
    }
}
impl Drop for Process {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
struct Root(PathBuf);
impl Drop for Root {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn forced_process_kill_recovers_durable_ranges_and_resumes_exact_bytes() {
    const MIB: u64 = 1024 * 1024;
    let root = Root(std::env::temp_dir().join(format!(
            "dm killed helper with spaces {}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("time")
                .as_nanos()
        )));
    fs::create_dir_all(root.0.join("Profile/Downloads")).expect("downloads");
    fs::create_dir_all(root.0.join("Local App Data")).expect("app data");
    // Expected SHA-256 independently computed with Python hashlib from the documented fixture formula.
    let fixture = Fixture {
        len: 8 * MIB,
        seed: 89,
    };
    let server = TestServer::start(ServerConfig {
        fixture: fixture.clone(),
        rules: vec![FaultRule {
            selector: RequestSelector {
                path: Some("/fixture".to_owned()),
                request_number: None,
                range: Some(ByteRange {
                    start: 7 * MIB,
                    end: 8 * MIB - 1,
                }),
            },
            fault: Fault::Stall(Duration::from_secs(4)),
        }],
    })
    .expect("server");
    let mut first = Process::start(&root.0);
    first.hello();
    first.send(
        "add",
        &json!({"url":server.url("/fixture"),"suggested_filename":"crash.bin","workers":4,"checksum":{"algorithm":"sha256","digest":"8da825cc025655c14fd604596e953db07bfdacdfa12361af4f89d67f00eaa934"}}),
    );
    let added = first.wait(|value| value["command"] == "add");
    assert_eq!(added["ok"], true);
    let task_id = added["result"]["task_id"].as_str().expect("id");
    let record = root
        .0
        .join("Local App Data/HalcyonXP/FirefoxDownloadManager/state/tasks")
        .join(format!("{task_id}.task.json"));
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        let value: Value =
            serde_json::from_slice(&fs::read(&record).expect("read checkpoint")).expect("record");
        if !value["task"]["completed_ranges"]
            .as_array()
            .expect("coverage")
            .is_empty()
        {
            break;
        }
        assert!(Instant::now() < deadline, "no durable range before kill");
        std::thread::sleep(Duration::from_millis(25));
    }
    assert!(!root.0.join("Profile/Downloads/crash.bin").exists());
    first.child.kill().expect("force kill");
    first.child.wait().expect("wait killed process");
    drop(first);
    let mut second = Process::start(&root.0);
    let snapshot = second.hello();
    assert_eq!(snapshot["data"]["tasks"][0]["state"], "paused");
    assert!(
        snapshot["data"]["tasks"][0]["bytes_completed"]
            .as_u64()
            .expect("retained bytes")
            > 0
    );
    second.send("resume", &json!({"task_id":task_id}));
    second.wait(|value| value["event"] == "completed");
    assert_eq!(
        fs::read(root.0.join("Profile/Downloads/crash.bin")).expect("final"),
        fixture.bytes(0, usize::try_from(fixture.len).expect("size"), 0)
    );
}
