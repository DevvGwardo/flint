use std::time::Duration;

use tokio::io::AsyncBufReadExt;
use tokio::io::AsyncReadExt;
use tokio::io::AsyncWriteExt;
use tokio::net::TcpListener;
use tokio_util::sync::CancellationToken;

use super::*;

/// A local-only endpoint that can deliberately leave the response unfinished.
async fn endpoint(
    response: String,
    stall: bool,
) -> (
    Provider,
    tokio::sync::oneshot::Receiver<()>,
    tokio::task::JoinHandle<()>,
) {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let url = format!("http://{}", listener.local_addr().expect("addr"));
    let (ready_tx, ready_rx) = tokio::sync::oneshot::channel();
    let server = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.expect("accept");
        let mut reader = tokio::io::BufReader::new(&mut socket);
        let mut length = 0;
        loop {
            let mut line = String::new();
            assert!(reader.read_line(&mut line).await.expect("header") > 0);
            if line == "\r\n" {
                break;
            }
            if let Some(value) = line.to_ascii_lowercase().strip_prefix("content-length:") {
                length = value.trim().parse::<usize>().expect("length");
            }
        }
        reader
            .read_exact(&mut vec![0u8; length])
            .await
            .expect("body");
        socket.write_all(response.as_bytes()).await.expect("write");
        let _ = ready_tx.send(());
        if stall {
            std::future::pending::<()>().await;
        }
    });
    (
        Provider::new(&url, "test-model", "fake-test-key"),
        ready_rx,
        server,
    )
}

fn request() -> ChatRequest<'static> {
    ChatRequest {
        messages: &[],
        replay_reasoning_from: 0,
        tools_json: "[]",
        effort: None,
    }
}

#[test]
fn debug_output_does_not_expose_the_api_key() {
    let provider = Provider::new("http://127.0.0.1", "test-model", "fake-test-key");
    assert!(!format!("{provider:?}").contains("fake-test-key"));
}

#[tokio::test]
async fn omniroute_compatible_auth_modes_preserve_catalog_routing_and_streamed_tools() {
    for key in ["", "router-fixture-key"] {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}/v1", listener.local_addr().unwrap());
        let server = tokio::spawn(async move {
            for index in 0..3 {
                let (mut socket, _) = listener.accept().await.unwrap();
                let mut reader = tokio::io::BufReader::new(&mut socket);
                let mut headers = String::new();
                let mut length = 0;
                loop {
                    let mut line = String::new();
                    assert!(reader.read_line(&mut line).await.unwrap() > 0);
                    if line == "\r\n" {
                        break;
                    }
                    if let Some(value) = line.to_ascii_lowercase().strip_prefix("content-length:") {
                        length = value.trim().parse::<usize>().unwrap();
                    }
                    headers.push_str(&line);
                }
                let headers = headers.to_ascii_lowercase();
                if key.is_empty() {
                    assert!(!headers.contains("authorization:"));
                } else {
                    assert!(headers.contains("authorization: bearer router-fixture-key"));
                }
                let mut body = vec![0; length];
                reader.read_exact(&mut body).await.unwrap();
                let (content_type, response) = if index < 2 {
                    assert!(headers.starts_with("get /v1/models "));
                    (
                        "application/json",
                        r#"{"data":[{"id":"auto","context_length":32768},{"id":"coding-combo"}]}"#,
                    )
                } else {
                    assert!(headers.starts_with("post /v1/chat/completions "));
                    let json: Value = serde_json::from_slice(&body).unwrap();
                    assert_eq!(json["model"], "auto");
                    assert_eq!(json["stream"], true);
                    (
                        "text/event-stream",
                        concat!(
                            "data: {\"choices\":[{\"delta\":{\"content\":\"Checking.\",\"tool_calls\":[",
                            "{\"index\":0,\"id\":\"route-call\",\"type\":\"function\",\"function\":{\"name\":\"read_file\",\"arguments\":\"{\\\"path\\\":\\\"README.md\\\"}\"}}",
                            "]},\"finish_reason\":\"tool_calls\"}]}\n\n",
                            "data: [DONE]\n\n",
                        ),
                    )
                };
                let response = format!(
                    "HTTP/1.1 200 OK\r\ncontent-type: {content_type}\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{response}",
                    response.len(),
                );
                socket.write_all(response.as_bytes()).await.unwrap();
            }
        });
        let provider = Provider::new(&base, "auto", key);
        assert_eq!(provider.model_limits().await.unwrap().context_window, 32768);
        let cancel = CancellationToken::new();
        assert_eq!(
            provider.list_models(&cancel).await.unwrap(),
            ["auto", "coding-combo"]
        );
        let completion = provider
            .complete(&request(), &mut |_| {}, &cancel)
            .await
            .unwrap();
        assert_eq!(completion.text, "Checking.");
        assert_eq!(completion.tool_calls.len(), 1);
        assert_eq!(completion.tool_calls[0].name, "read_file");
        server.await.unwrap();
    }
}

#[tokio::test]
async fn eof_and_unsupported_terminal_outcomes_cannot_release_tools() {
    for reason in [
        None,
        Some("length"),
        Some("content_filter"),
        Some("unexpected"),
    ] {
        let mut body = concat!(
            "data: {\"choices\":[{\"delta\":{\"content\":\"partial\",\"tool_calls\":[",
            "{\"id\":\"c\",\"function\":{\"name\":\"write_file\",\"arguments\":\"{}\"}}]}}]}\n\n"
        )
        .to_string();
        if let Some(reason) = reason {
            body.push_str(&format!(
                "data: {{\"choices\":[{{\"delta\":{{}},\"finish_reason\":\"{reason}\"}}]}}\n\n"
            ));
            body.push_str("data: [DONE]\n\n");
        }
        let response = format!(
            "HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\ncontent-length: {}\r\n\r\n{body}",
            body.len()
        );
        let (provider, _, server) = endpoint(response, false).await;
        let result = provider
            .complete(&request(), &mut |_| {}, &CancellationToken::new())
            .await;
        server.await.expect("server");
        assert!(result.is_err(), "unsafe completion accepted: {reason:?}");
    }
}

#[tokio::test]
async fn supported_finish_allows_clean_eof_without_done() {
    let body = "data: {\"choices\":[{\"delta\":{\"content\":\"complete\"},\"finish_reason\":\"stop\"}]}\n\n";
    let response = format!(
        "HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\ncontent-length: {}\r\n\r\n{body}",
        body.len()
    );
    let (provider, _, server) = endpoint(response, false).await;
    assert!(
        provider
            .complete(&request(), &mut |_| {}, &CancellationToken::new())
            .await
            .is_ok()
    );
    server.await.expect("server");
}

#[tokio::test]
async fn done_stops_processing_the_rest_of_the_body() {
    let body = concat!(
        "data: {\"choices\":[{\"delta\":{\"content\":\"kept\"},\"finish_reason\":\"stop\"}]}\n\n",
        "data: [DONE]\n\n",
        "data: {\"error\":{\"message\":\"must be ignored\"}}\n\n",
    );
    let response = format!(
        "HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\ncontent-length: {}\r\n\r\n{body}",
        body.len()
    );
    let (provider, _, server) = endpoint(response, false).await;
    let result = provider
        .complete(&request(), &mut |_| {}, &CancellationToken::new())
        .await;
    server.await.expect("server");
    assert_eq!(result.expect("completion").text, "kept");
}

#[tokio::test]
async fn cancellation_interrupts_a_stalled_error_body() {
    let response = "HTTP/1.1 401 Unauthorized\r\ncontent-length: 10000\r\n\r\n".to_string();
    let (provider, ready, server) = endpoint(response, true).await;
    let cancel = CancellationToken::new();
    let trigger = cancel.clone();
    let canceller = tokio::spawn(async move {
        ready.await.expect("response headers");
        tokio::time::sleep(Duration::from_millis(50)).await;
        trigger.cancel();
    });
    let result = tokio::time::timeout(
        Duration::from_secs(2),
        provider.complete(&request(), &mut |_| {}, &cancel),
    )
    .await;
    server.abort();
    canceller.await.expect("canceller");
    assert_eq!(
        result.expect("cancellation in time"),
        Err(ProviderError::Cancelled)
    );
}

#[tokio::test]
async fn error_bodies_are_bounded_without_waiting_for_the_rest() {
    let response = format!(
        "HTTP/1.1 401 Unauthorized\r\ncontent-length: 1048576\r\n\r\n{}",
        "x".repeat(4096)
    );
    let (provider, _, server) = endpoint(response, true).await;
    let result = tokio::time::timeout(
        Duration::from_secs(2),
        provider.complete(&request(), &mut |_| {}, &CancellationToken::new()),
    )
    .await;
    server.abort();
    let Err(ProviderError::Failed(message)) = result.expect("bounded error read") else {
        panic!("expected the HTTP error");
    };
    assert!(message.contains("401"));
    assert!(message.len() < 600);
}
