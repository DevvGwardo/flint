//! Loopback-only provider. Never reads environment credentials or records requests.
use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};
use std::time::Duration;

pub struct LocalProvider {
    pub url: String,
    pub model_requests: Arc<AtomicUsize>,
    pub completion_requests: Arc<AtomicUsize>,
    stop: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl LocalProvider {
    pub fn approval() -> Self {
        Self::listing(Duration::ZERO)
    }

    pub fn listing(delay: Duration) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/v1", listener.local_addr().unwrap());
        listener.set_nonblocking(true).unwrap();
        let stop = Arc::new(AtomicBool::new(false));
        let stopped = stop.clone();
        let model_requests = Arc::new(AtomicUsize::new(0));
        let completion_requests = Arc::new(AtomicUsize::new(0));
        let models = model_requests.clone();
        let completions = completion_requests.clone();
        let thread = std::thread::spawn(move || {
            while !stopped.load(Ordering::Relaxed) {
                let Ok((mut socket, _)) = listener.accept() else {
                    std::thread::sleep(Duration::from_millis(2));
                    continue;
                };
                // macOS can inherit the listener's nonblocking flag. Wait for
                // the request instead of treating WouldBlock as an empty POST.
                socket.set_nonblocking(false).unwrap();
                socket
                    .set_read_timeout(Some(Duration::from_secs(1)))
                    .unwrap();
                let mut request = Vec::new();
                let mut buf = [0; 4096];
                while let Ok(n) = socket.read(&mut buf) {
                    if n == 0 {
                        break;
                    }
                    request.extend_from_slice(&buf[..n]);
                    if request.windows(4).any(|s| s == b"\r\n\r\n") {
                        break;
                    }
                }
                let (kind, body) = if request.starts_with(b"GET /v1/models ") {
                    models.fetch_add(1, Ordering::Relaxed);
                    std::thread::sleep(delay);
                    (
                        "application/json",
                        r#"{"data":[{"id":"fixture-model","context_length":128000}]}"#.to_string(),
                    )
                } else if request.starts_with(b"POST /v1/chat/completions ") {
                    completions.fetch_add(1, Ordering::Relaxed);
                    let delta = serde_json::json!({"choices":[{"delta":{"tool_calls":[{
                        "index":0,"id":"fixture-edit","type":"function",
                        "function":{"name":"write_file","arguments":"{\"path\":\"fixture.txt\",\"content\":\"fixture\\n\"}"}
                    }]}}]});
                    (
                        "text/event-stream",
                        format!("data: {delta}\n\ndata: [DONE]\n\n"),
                    )
                } else {
                    let _ = socket.write_all(
                        b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                    );
                    continue;
                };
                let reply = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: {kind}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = socket.write_all(reply.as_bytes());
            }
        });
        Self {
            url,
            model_requests,
            completion_requests,
            stop,
            thread: Some(thread),
        }
    }
}

impl Drop for LocalProvider {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}
