use std::time::{Duration, Instant};

use download_manager_engine::admission::{Admission, AdmissionError};
use download_manager_engine::network::ProbeClient;
use download_manager_engine::scheduler::ConcurrencyLimits;
use download_manager_test_server::{Fault, FaultRule, RequestSelector, ServerConfig, TestServer};
use reqwest::Url;

fn url(origin: &str) -> Url {
    Url::parse(origin).expect("fixture URL")
}

#[tokio::test]
async fn cancellation_of_waiting_admission_never_leaks_global_or_origin_capacity() {
    let admission = Admission::new(ConcurrencyLimits::new(1, 1).expect("limits"));
    let a = url("https://a.example.test/file");
    let b = url("https://b.example.test/file");
    let held = admission.acquire(&a).await.expect("first request");
    assert!(
        tokio::time::timeout(Duration::from_millis(20), admission.acquire(&b))
            .await
            .is_err()
    );
    assert_eq!(admission.active(), 1);
    drop(held);
    let next = tokio::time::timeout(Duration::from_millis(100), admission.acquire(&b))
        .await
        .expect("no leaked global slot")
        .expect("request");
    assert_eq!(admission.peak(), 1);
    drop(next);
    assert_eq!(admission.active(), 0);
}

#[tokio::test]
async fn rate_guidance_delays_peers_and_halves_origin_pressure_without_blocking_others() {
    for status in [429, 503] {
        let admission = Admission::new(ConcurrencyLimits::new(4, 4).expect("limits"));
        let a = url("https://a.example.test/file");
        let b = url("https://b.example.test/file");
        let first = admission.acquire(&a).await.expect("first");
        let started = Instant::now();
        first.observe(status, Some(1));
        drop(first);
        assert!(
            tokio::time::timeout(Duration::from_millis(30), admission.acquire(&a))
                .await
                .is_err()
        );
        let other = tokio::time::timeout(Duration::from_millis(100), admission.acquire(&b))
            .await
            .expect("other origin proceeds")
            .expect("other request");
        let a1 = tokio::time::timeout(Duration::from_secs(3), admission.acquire(&a))
            .await
            .expect("bounded guidance")
            .expect("after guidance");
        assert!(started.elapsed() >= Duration::from_secs(1));
        let a2 = admission.acquire(&a).await.expect("reduced cap is two");
        assert!(
            tokio::time::timeout(Duration::from_millis(20), admission.acquire(&a))
                .await
                .is_err()
        );
        assert_eq!(admission.active(), 3);
        a1.observe(status, Some(0));
        drop(a1);
        assert!(
            tokio::time::timeout(Duration::from_millis(20), admission.acquire(&a))
                .await
                .is_err()
        );
        drop(a2);
        let final_request = admission.acquire(&a).await.expect("reduced cap is one");
        drop(final_request);
        drop(other);
        assert_eq!(admission.active(), 0);
        assert!(admission.peak() <= 4);
    }
}

#[tokio::test]
async fn excessive_retry_after_blocks_instead_of_retrying_early() {
    let admission = Admission::new(ConcurrencyLimits::default());
    let a = url("https://a.example.test/file");
    let first = admission.acquire(&a).await.expect("request");
    first.observe(503, Some(3601));
    drop(first);
    assert!(matches!(
        admission.acquire(&a).await,
        Err(AdmissionError::ServerDelay)
    ));
    let other = admission
        .acquire(&url("https://other.example.test/file"))
        .await
        .expect("other origin");
    assert!(matches!(
        admission.acquire(&a).await,
        Err(AdmissionError::ServerDelay)
    ));
    drop(other);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn probe_requests_and_redirect_hops_share_the_same_global_admission() {
    let target = TestServer::start(ServerConfig::default()).expect("target");
    let source = TestServer::start(ServerConfig {
        rules: vec![FaultRule {
            selector: RequestSelector::default(),
            fault: Fault::Redirect(target.url("/fixture")),
        }],
        ..ServerConfig::default()
    })
    .expect("source");
    let admission = Admission::new(ConcurrencyLimits::new(1, 1).expect("limits"));
    let client = ProbeClient::with_admission(admission.clone()).expect("client");
    let barrier = target.pause_observation();
    let first_client = client.clone();
    let first_url = source.url("/fixture");
    let first = tokio::spawn(async move { first_client.probe(&first_url).await });
    assert!(barrier.wait_for_pending(1, Duration::from_secs(3)));
    assert_eq!(admission.active(), 1);
    let second_client = client.clone();
    let second_url = target.url("/fixture");
    let second = tokio::spawn(async move { second_client.probe(&second_url).await });
    assert!(!barrier.wait_for_pending(2, Duration::from_millis(100)));
    assert_eq!(admission.peak(), 1);
    drop(barrier);
    first.await.expect("first join").expect("redirected probe");
    second.await.expect("second join").expect("second probe");
    assert_eq!(target.requests().len(), 4);
    assert_eq!(admission.peak(), 1);
    assert_eq!(admission.active(), 0);
}
