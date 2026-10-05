//! `fetch_url`: GET a web page or file over http(s) and return it as text.
//! HTML is reduced to readable text (scripts, styles and tags removed).
//! The body read and the text the model sees are both bounded.

use std::time::Duration;

use futures_util::StreamExt;
use serde_json::Map;
use serde_json::Value;
use tokio_util::sync::CancellationToken;

use super::ToolOutcome;
use super::head_tail;
use super::str_arg;

const TIMEOUT: Duration = Duration::from_secs(30);
/// Bytes of the body read; the rest is not downloaded.
const MAX_BODY_BYTES: usize = 3 * 1024 * 1024;
/// Characters of text the model sees.
const MODEL_CHARS: usize = 20_000;

/// The host of an http(s) URL, for approval rules and summaries.
pub fn url_host(url: &str) -> Option<String> {
    let parsed = reqwest::Url::parse(url.trim()).ok()?;
    matches!(parsed.scheme(), "http" | "https")
        .then(|| parsed.host_str().map(str::to_ascii_lowercase))
        .flatten()
}

pub(super) async fn fetch_url(
    args: &Map<String, Value>,
    cancel: &CancellationToken,
) -> ToolOutcome {
    let Some(url) = str_arg(args, "url")
        .map(str::trim)
        .filter(|u| !u.is_empty())
    else {
        return ToolOutcome::error("`url` is required");
    };
    if url_host(url).is_none() {
        return ToolOutcome::error(format!("`{url}` is not an http(s) URL"));
    }
    let client = match reqwest::Client::builder()
        .timeout(TIMEOUT)
        .redirect(reqwest::redirect::Policy::none())
        .user_agent(concat!("flint/", env!("CARGO_PKG_VERSION")))
        .build()
    {
        Ok(client) => client,
        Err(err) => return ToolOutcome::error(format!("cannot start the HTTP client: {err}")),
    };
    let request = client.get(url).send();
    let response = tokio::select! {
        response = request => match response {
            Ok(response) => response,
            Err(err) => return ToolOutcome::error(format!("fetch failed: {err}")),
        },
        () = cancel.cancelled() => return ToolOutcome::error("Interrupted."),
    };
    let status = response.status();
    if status.is_redirection() {
        return ToolOutcome::error(
            "Redirect refused: fetch the destination URL separately so its host can be approved.",
        );
    }
    let final_url = response.url().to_string();
    let content_type = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_ascii_lowercase();
    let mut body = Vec::new();
    let mut truncated = false;
    let mut stream = response.bytes_stream();
    loop {
        let chunk = tokio::select! {
            chunk = stream.next() => chunk,
            () = cancel.cancelled() => return ToolOutcome::error("Interrupted."),
        };
        match chunk {
            None => break,
            Some(Err(err)) => return ToolOutcome::error(format!("fetch failed: {err}")),
            Some(Ok(bytes)) => {
                let room = MAX_BODY_BYTES - body.len();
                body.extend_from_slice(&bytes[..bytes.len().min(room)]);
                if bytes.len() > room {
                    truncated = true;
                    break;
                }
            }
        }
    }
    let is_text = content_type.is_empty()
        || content_type.starts_with("text/")
        || ["json", "xml", "javascript", "yaml", "toml", "csv"]
            .iter()
            .any(|kind| content_type.contains(kind));
    if !is_text || body.contains(&0) {
        return ToolOutcome::error(format!(
            "{final_url} returned {content_type}, which is not text ({} bytes)",
            body.len()
        ));
    }
    let raw = String::from_utf8_lossy(&body);
    let text = if content_type.contains("html") || looks_like_html(&raw) {
        html_to_text(&raw)
    } else {
        raw.into_owned()
    };
    let mut output = format!("{final_url} — HTTP {}\n\n", status.as_u16());
    output.push_str(&head_tail(text.trim(), MODEL_CHARS));
    if truncated {
        output.push_str("\n[body truncated after 3 MB]");
    }
    ToolOutcome {
        output,
        exit_code: None,
        success: status.is_success(),
        diff: None,
    }
}

fn looks_like_html(text: &str) -> bool {
    let start: String = text
        .trim_start()
        .chars()
        .take(200)
        .collect::<String>()
        .to_ascii_lowercase();
    start.starts_with("<!doctype html") || start.starts_with("<html")
}

/// Readable text from HTML: drops scripts, styles and tags, keeps line
/// breaks at block elements, decodes common entities, drops blank lines.
pub fn html_to_text(html: &str) -> String {
    let mut out = String::with_capacity(html.len() / 3);
    let lower = html.to_ascii_lowercase();
    let mut i = 0;
    let bytes = html.as_bytes();
    while i < html.len() {
        if bytes[i] == b'<' {
            let Some(close) = html[i..].find('>') else {
                break;
            };
            let tag = &lower[i + 1..i + close];
            let name: String = tag
                .trim_start_matches('/')
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric())
                .collect();
            i += close + 1;
            if !tag.starts_with('/')
                && matches!(
                    name.as_str(),
                    "script" | "style" | "noscript" | "svg" | "head"
                )
            {
                let end = format!("</{name}");
                match lower[i..].find(&end) {
                    Some(at) => {
                        i += at;
                        i += lower[i..].find('>').map_or(lower.len() - i, |c| c + 1);
                    }
                    None => break,
                }
                continue;
            }
            if matches!(
                name.as_str(),
                "p" | "div"
                    | "br"
                    | "li"
                    | "tr"
                    | "h1"
                    | "h2"
                    | "h3"
                    | "h4"
                    | "h5"
                    | "h6"
                    | "pre"
                    | "section"
                    | "article"
                    | "header"
                    | "footer"
                    | "table"
                    | "ul"
                    | "ol"
                    | "blockquote"
            ) {
                out.push('\n');
            }
            if name == "li" && !tag.starts_with('/') {
                out.push_str("- ");
            }
            continue;
        }
        let next = html[i..].find('<').map_or(html.len(), |at| i + at);
        out.push_str(&decode_entities(&html[i..next]));
        i = next;
    }
    out.lines()
        .map(|line| line.split_whitespace().collect::<Vec<_>>().join(" "))
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

fn decode_entities(text: &str) -> String {
    text.replace("&nbsp;", " ")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&apos;", "'")
        .replace("&amp;", "&")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn html_becomes_readable_text() {
        let html = "<!DOCTYPE html><html><head><title>T</title><style>p{}</style></head>\
            <body><script>alert(1)</script><h1>Title &amp; more</h1><p>One <b>bold</b> line.</p>\
            <ul><li>a</li><li>b</li></ul></body></html>";
        assert_eq!(html_to_text(html), "Title & more\nOne bold line.\n- a\n- b");
    }

    #[test]
    fn hosts_are_only_for_http_urls() {
        assert_eq!(url_host("https://Docs.rs/serde"), Some("docs.rs".into()));
        assert_eq!(url_host("file:///etc/passwd"), None);
        assert_eq!(url_host("not a url"), None);
    }

    #[tokio::test]
    async fn redirects_are_not_followed_to_an_unapproved_host() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let destination = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind");
        let destination_url = format!(
            "http://localhost:{}/private",
            destination.local_addr().expect("addr").port()
        );
        let source = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind");
        let source_url = format!("http://{}", source.local_addr().expect("addr"));
        let server = tokio::spawn(async move {
            let (mut socket, _) = source.accept().await.expect("accept");
            let mut buf = [0; 4096];
            assert!(socket.read(&mut buf).await.expect("request") > 0);
            socket.write_all(format!(
                "HTTP/1.1 302 Found\r\nLocation: {destination_url}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
            ).as_bytes()).await.expect("redirect");
        });
        let mut args = Map::new();
        args.insert("url".into(), Value::String(source_url));
        let outcome = tokio::time::timeout(
            Duration::from_secs(2),
            fetch_url(&args, &CancellationToken::new()),
        )
        .await
        .expect("fetch");
        assert!(!outcome.success);
        assert!(outcome.output.contains("Redirect refused"));
        assert!(
            tokio::time::timeout(Duration::from_millis(100), destination.accept())
                .await
                .is_err()
        );
        server.await.expect("server");
    }
}
