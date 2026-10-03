use super::*;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

/// Discards headers/body immediately. Only counts a loopback GET with no body.
fn mock(
    status: u16,
    body: &str,
    extra: &str,
    delay: Duration,
) -> (String, Arc<AtomicUsize>, std::thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}/v1", listener.local_addr().unwrap());
    let calls = Arc::new(AtomicUsize::new(0));
    let seen = calls.clone();
    let body = body.to_string();
    let extra = extra.to_string();
    let worker = std::thread::spawn(move || {
        let (mut socket, _) = listener.accept().unwrap();
        socket
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        let mut buf = [0; 4096];
        let n = socket.read(&mut buf).unwrap_or(0);
        assert!(buf[..n].starts_with(b"GET /v1/models "));
        seen.fetch_add(1, Ordering::Relaxed);
        std::thread::sleep(delay);
        let response = format!(
            "HTTP/1.1 {status} Fixture\r\nContent-Length: {}\r\nConnection: close\r\n{extra}\r\n{body}",
            body.len()
        );
        let _ = socket.write_all(response.as_bytes());
    });
    (url, calls, worker)
}

#[tokio::test]
async fn model_listing_is_not_authentication_or_inference_proof() {
    for key in [None, Some("dummy-only")] {
        let (url, calls, worker) = mock(200, r#"{"data":[{"id":"fixture"}]}"#, "", Duration::ZERO);
        let result = check(
            models_url(&url).unwrap(),
            "fixture",
            key,
            Duration::from_secs(1),
        )
        .await;
        assert!(result.reachable);
        assert_eq!(result.credentials_accepted, None);
        assert_eq!(result.model_listed, Some(true));
        assert!(result.label().contains("Inference was not tested"));
        worker.join().unwrap();
        assert_eq!(calls.load(Ordering::Relaxed), 1);
    }
}

#[tokio::test]
async fn missing_model_auth_failure_unsupported_and_malformed_are_distinct() {
    for (status, body, reachable, auth, model) in [
        (200, r#"{"data":[]}"#, true, None, Some(false)),
        (401, "must never display this body", true, Some(false), None),
        (403, "", true, Some(false), None),
        (404, "", true, None, None),
        (405, "", true, None, None),
        (501, "", true, None, None),
        (500, "", true, None, None),
        (200, "not json", true, None, None),
        (200, r#"{"models":[]}"#, true, None, None),
        (200, r#"{"data":[{}]}"#, true, None, None),
    ] {
        let (url, _, worker) = mock(status, body, "", Duration::ZERO);
        let result = check(
            models_url(&url).unwrap(),
            "fixture",
            Some("dummy"),
            Duration::from_secs(1),
        )
        .await;
        assert_eq!(
            (
                result.reachable,
                result.credentials_accepted,
                result.model_listed
            ),
            (reachable, auth, model)
        );
        assert!(!result.label().contains("must never display"));
        worker.join().unwrap();
    }
}

#[tokio::test]
async fn redirects_never_contact_the_other_origin() {
    let destination = TcpListener::bind("127.0.0.1:0").unwrap();
    destination.set_nonblocking(true).unwrap();
    let extra = format!(
        "Location: http://{}/v1/models\r\n",
        destination.local_addr().unwrap()
    );
    let (url, _, worker) = mock(302, "", &extra, Duration::ZERO);
    let result = check(
        models_url(&url).unwrap(),
        "fixture",
        Some("dummy"),
        Duration::from_secs(1),
    )
    .await;
    assert!(result.reachable && result.message.contains("Redirect blocked"));
    worker.join().unwrap();
    assert_eq!(
        destination.accept().unwrap_err().kind(),
        std::io::ErrorKind::WouldBlock
    );
}

#[tokio::test]
async fn timeout_and_response_size_are_bounded() {
    let (url, _, worker) = mock(200, "{}", "", Duration::from_millis(100));
    let result = check(
        models_url(&url).unwrap(),
        "fixture",
        None,
        Duration::from_millis(25),
    )
    .await;
    assert!(result.message.contains("timed out"));
    worker.join().unwrap();
    let (url, _, worker) = mock(200, &" ".repeat(1024 * 1024 + 1), "", Duration::ZERO);
    let result = check(
        models_url(&url).unwrap(),
        "fixture",
        None,
        Duration::from_secs(2),
    )
    .await;
    assert!(result.message.contains("1 MiB"));
    worker.join().unwrap();
}

#[test]
fn cancelling_probe_aborts_inflight_request_and_closes_result_channel() {
    let (url, calls, worker) = mock(200, "{}", "", Duration::from_millis(200));
    let probe = Probe::start(&url, "fixture".into(), Some("dummy".into())).unwrap();
    let receiver = probe.result.clone();
    let deadline = std::time::Instant::now() + Duration::from_secs(2);
    while calls.load(Ordering::Relaxed) == 0 {
        assert!(std::time::Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(2));
    }
    drop(probe);
    while !receiver.is_closed() {
        assert!(std::time::Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(2));
    }
    assert!(receiver.try_recv().is_err());
    worker.join().unwrap();
}

#[test]
fn url_validation_never_accepts_embedded_credentials() {
    for url in [
        "https://user:dummy@localhost/v1",
        "https://localhost/v1?key=dummy",
        "https://localhost/#key",
        "file:///tmp/file",
        "http://",
    ] {
        assert!(models_url(url).is_err());
    }
    assert_eq!(
        models_url("http://localhost:123/v1/").unwrap().path(),
        "/v1/models"
    );
}

#[tokio::test]
async fn no_credentials_cannot_be_reported_as_rejected_credentials() {
    for status in [401, 403] {
        let (url, _, worker) = mock(status, "", "", Duration::ZERO);
        let result = check(
            models_url(&url).unwrap(),
            "fixture",
            None,
            Duration::from_secs(1),
        )
        .await;
        assert_eq!(result.credentials_accepted, None);
        assert!(result.message.contains("no credential was tested"));
        worker.join().unwrap();
    }
}
