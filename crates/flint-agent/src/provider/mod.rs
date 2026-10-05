//! Chat Completions client: streaming, retries, and the `reasoning_effort`
//! fallback. Request bodies are built in [`wire`].

pub mod stream;
pub mod wire;

use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::time::Duration;
use std::time::Instant;

use bytes::Bytes;
use futures_util::StreamExt;
use serde_json::Value;
use tokio_util::sync::CancellationToken;

pub use stream::Completion;
use stream::CompletionBuilder;
pub use stream::RawToolCall;
use stream::SseParser;
pub use stream::StreamDelta;
pub use stream::Timing;
pub use wire::ChatRequest;
pub use wire::Message;

/// Retries for dropped connections and server errors: a dead endpoint
/// should fail fast.
const MAX_RETRIES: u32 = 3;
/// Retries when the endpoint says it is busy (429/503): those pass.
const MAX_THROTTLED_RETRIES: u32 = 5;
/// Longest wait between retries, whatever the backoff or `Retry-After` say.
const MAX_RETRY_WAIT: Duration = Duration::from_secs(60);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(20);
/// Longest silence allowed mid-stream before the request is abandoned.
const IDLE_TIMEOUT: Duration = Duration::from_secs(180);
/// Keep pooled connections warm between steps (steps are seconds apart).
const POOL_IDLE_TIMEOUT: Duration = Duration::from_secs(300);
/// Enough UTF-8 bytes for the 500-character HTTP error snippet.
const MAX_ERROR_BYTES: usize = 2000;
const MAX_STREAM_BYTES: usize = 16 * 1024 * 1024;
/// Idle timeout remains unchanged; a drip-feed must not run forever.
const STREAM_DEADLINE: Duration = Duration::from_secs(900);

/// Why a model call failed.
#[derive(Debug, Clone, PartialEq)]
pub enum ProviderError {
    Cancelled,
    /// The stream broke after part of the reply was shown, so the call was
    /// not retried here (the shown part would repeat).
    Dropped(String),
    /// Retries exhausted or a non-retryable error.
    Failed(String),
}

/// An OpenAI-compatible Chat Completions endpoint. One client (and its
/// keep-alive connection pool) is reused for every call in a session.
pub struct Provider {
    http: reqwest::Client,
    url: String,
    /// `{base}/models`, for [`Provider::model_limits`].
    models_url: String,
    model: String,
    api_key: String,
    /// Set once the endpoint rejected `reasoning_effort`; it is not sent again.
    effort_rejected: AtomicBool,
}

impl std::fmt::Debug for Provider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Provider")
            .field("url", &self.url)
            .field("model", &self.model)
            .field("effort_rejected", &self.effort_rejected)
            .finish_non_exhaustive()
    }
}

impl Provider {
    pub fn new(base_url: &str, model: &str, api_key: &str) -> Self {
        let http = reqwest::Client::builder()
            .connect_timeout(CONNECT_TIMEOUT)
            .pool_idle_timeout(POOL_IDLE_TIMEOUT)
            .tcp_nodelay(true)
            .build()
            .unwrap_or_default();
        Self {
            http,
            url: format!("{}/chat/completions", base_url.trim_end_matches('/')),
            models_url: format!("{}/models", base_url.trim_end_matches('/')),
            model: model.to_string(),
            api_key: api_key.to_string(),
            effort_rejected: AtomicBool::new(false),
        }
    }

    /// The model's context window and output cap, from the endpoint's
    /// `GET /models` listing. Reads the fields OpenAI-compatible servers use:
    /// `context_length` / `top_provider.context_length` (OpenRouter and
    /// gateways like it), `max_model_len` (vLLM), `context_window`, and
    /// `max_completion_tokens` / `top_provider.max_completion_tokens`.
    /// `None` when the listing is unavailable or doesn't say.
    pub async fn model_limits(&self) -> Option<ModelLimits> {
        let response = self
            .http
            .get(&self.models_url)
            .bearer_auth(&self.api_key)
            .timeout(Duration::from_secs(10))
            .send()
            .await
            .ok()?;
        if !response.status().is_success() {
            return None;
        }
        let listing: serde_json::Value = response.json().await.ok()?;
        model_limits_from_listing(&listing, &self.model)
    }

    /// Exact model ids advertised by the configured endpoint.
    pub async fn list_models(&self, cancel: &CancellationToken) -> Result<Vec<String>, String> {
        let request = self
            .http
            .get(&self.models_url)
            .bearer_auth(&self.api_key)
            .timeout(Duration::from_secs(10))
            .send();
        let response = tokio::select! {
            result = request => result.map_err(|_| "Cannot fetch the endpoint's model list.".to_string())?,
            () = cancel.cancelled() => return Err("Interrupted.".to_string()),
        };
        if !response.status().is_success() {
            return Err(format!("Model listing returned {}.", response.status()));
        }
        let listing: Value = tokio::select! {
            result = response.json() => result.map_err(|_| "Invalid model listing.".to_string())?,
            () = cancel.cancelled() => return Err("Interrupted.".to_string()),
        };
        let entries = listing
            .get("data")
            .and_then(Value::as_array)
            .ok_or("Model listing has no data array.")?;
        let mut ids: Vec<String> = entries
            .iter()
            .filter_map(|entry| entry.get("id").and_then(Value::as_str))
            .filter(|id| !id.trim().is_empty())
            .map(str::to_string)
            .collect();
        ids.sort();
        ids.dedup();
        Ok(ids)
    }

    /// False once the endpoint rejected `reasoning_effort` in this session.
    pub fn effort_supported(&self) -> bool {
        !self.effort_rejected.load(Ordering::Relaxed)
    }

    /// Streams one completion. `on_delta` sees text/reasoning as it arrives.
    ///
    /// Retries rate limits, server errors and dropped connections, but only
    /// before any delta was shown. A 400/422 for a request carrying
    /// `reasoning_effort` is retried once without it; if that works, the
    /// parameter is dropped for the session (see [`Provider::effort_supported`]).
    pub async fn complete(
        &self,
        request: &ChatRequest<'_>,
        on_delta: &mut (dyn FnMut(StreamDelta) + Send),
        cancel: &CancellationToken,
    ) -> Result<Completion, ProviderError> {
        let mut effort = request.effort.filter(|_| self.effort_supported());
        let mut body = Bytes::from(ChatRequest { effort, ..*request }.body(&self.model));
        let mut attempt = 0;
        loop {
            let mut shown = false;
            match self
                .attempt(body.clone(), on_delta, &mut shown, cancel)
                .await
            {
                Ok(completion) => return Ok(completion),
                Err(Attempt::Cancelled) => return Err(ProviderError::Cancelled),
                Err(Attempt::Rejected(message)) if effort.is_some() => {
                    effort = None;
                    body = Bytes::from(ChatRequest { effort, ..*request }.body(&self.model));
                    let mut retried_shown = false;
                    return match self
                        .attempt(body, on_delta, &mut retried_shown, cancel)
                        .await
                    {
                        Ok(completion) => {
                            self.effort_rejected.store(true, Ordering::Relaxed);
                            Ok(completion)
                        }
                        Err(Attempt::Cancelled) => Err(ProviderError::Cancelled),
                        Err(
                            Attempt::Rejected(_)
                            | Attempt::Retryable(_)
                            | Attempt::Throttled(..)
                            | Attempt::Fatal(_),
                        ) => Err(ProviderError::Failed(message)),
                    };
                }
                Err(Attempt::Rejected(message) | Attempt::Fatal(message)) => {
                    return Err(ProviderError::Failed(message));
                }
                Err(failure @ (Attempt::Retryable(_) | Attempt::Throttled(..))) => {
                    let (message, retry_after, limit) = match failure {
                        Attempt::Throttled(message, after) => {
                            (message, after, MAX_THROTTLED_RETRIES)
                        }
                        Attempt::Retryable(message) => (message, None, MAX_RETRIES),
                        Attempt::Cancelled | Attempt::Rejected(_) | Attempt::Fatal(_) => {
                            unreachable!("matched above")
                        }
                    };
                    attempt += 1;
                    if shown {
                        return Err(ProviderError::Dropped(message));
                    }
                    if attempt > limit {
                        return Err(ProviderError::Failed(message));
                    }
                    let backoff = retry_after
                        .unwrap_or_else(|| Duration::from_millis(1000 * 2u64.pow(attempt - 1)))
                        .min(MAX_RETRY_WAIT);
                    tokio::select! {
                        () = tokio::time::sleep(backoff) => {}
                        () = cancel.cancelled() => return Err(ProviderError::Cancelled),
                    }
                }
            }
        }
    }

    async fn attempt(
        &self,
        body: Bytes,
        on_delta: &mut (dyn FnMut(StreamDelta) + Send),
        shown: &mut bool,
        cancel: &CancellationToken,
    ) -> Result<Completion, Attempt> {
        let started = Instant::now();
        let request = self
            .http
            .post(&self.url)
            .bearer_auth(&self.api_key)
            .header(reqwest::header::CONTENT_TYPE, "application/json")
            .body(body)
            .send();
        let response = tokio::select! {
            response = tokio::time::timeout(IDLE_TIMEOUT, request) =>
                response.map_err(|_| Attempt::Retryable("response headers stalled".to_string()))?
                    .map_err(|err| Attempt::Retryable(format!("request failed: {err}")))?,
            () = cancel.cancelled() => return Err(Attempt::Cancelled),
        };
        let status = response.status();
        if !status.is_success() {
            let retry_after = response
                .headers()
                .get(reqwest::header::RETRY_AFTER)
                .and_then(|value| value.to_str().ok())
                .and_then(parse_retry_after);
            let snippet = error_snippet(response, cancel).await?;
            let message = format!("provider returned {status}: {snippet}");
            return Err(match status.as_u16() {
                429 | 503 => Attempt::Throttled(message, retry_after),
                500..=599 => Attempt::Retryable(message),
                400 | 422 => Attempt::Rejected(message),
                _ => Attempt::Fatal(message),
            });
        }
        let mut timing = Timing {
            headers: Some(started.elapsed()),
            ..Timing::default()
        };
        let mut bytes = response.bytes_stream();
        let mut parser = SseParser::default();
        let mut builder = CompletionBuilder::default();
        let mut done = false;
        let mut received = 0usize;
        let deadline = tokio::time::Instant::from_std(started) + STREAM_DEADLINE;
        while !done {
            let next = tokio::select! {
                next = tokio::time::timeout(IDLE_TIMEOUT, bytes.next()) => next,
                () = cancel.cancelled() => return Err(Attempt::Cancelled),
                () = tokio::time::sleep_until(deadline) =>
                    return Err(Attempt::Retryable("stream exceeded the total deadline".into())),
            };
            let chunk = match next {
                Err(_) => return Err(Attempt::Retryable("stream stalled".to_string())),
                Ok(None) => break,
                Ok(Some(Err(err))) => {
                    return Err(Attempt::Retryable(format!("stream broke: {err}")));
                }
                Ok(Some(Ok(chunk))) => chunk,
            };
            timing.first_byte.get_or_insert_with(|| started.elapsed());
            received = received.saturating_add(chunk.len());
            if received > MAX_STREAM_BYTES {
                return Err(Attempt::Fatal(
                    "provider stream exceeded the 16 MiB limit".into(),
                ));
            }
            for payload in parser.push(&chunk) {
                done = handle_payload(&payload, &mut builder, on_delta, shown)?;
                if *shown {
                    timing.first_delta.get_or_insert_with(|| started.elapsed());
                }
                if done {
                    break;
                }
            }
            if parser.exceeded() || builder.exceeded() {
                return Err(Attempt::Fatal(
                    "provider stream exceeded a parsing/completion limit".into(),
                ));
            }
        }
        if !done && let Some(payload) = parser.finish() {
            handle_payload(&payload, &mut builder, on_delta, shown)?;
        }
        if parser.exceeded() || builder.exceeded() {
            return Err(Attempt::Fatal(
                "provider stream exceeded a parsing/completion limit".into(),
            ));
        }
        timing.total = started.elapsed();
        let mut completion = builder.finish();
        match completion.finish_reason.as_deref() {
            Some("stop" | "tool_calls") => {}
            None => {
                return Err(Attempt::Retryable(
                    "stream ended without a terminal finish_reason".into(),
                ));
            }
            Some(reason) => {
                return Err(Attempt::Fatal(format!(
                    "incomplete or unsupported model outcome: {reason}; no tools were executed"
                )));
            }
        }
        completion.timing = timing;
        Ok(completion)
    }
}

/// Do not download an entire error page, or lose interruption while reading it.
async fn error_snippet(
    response: reqwest::Response,
    cancel: &CancellationToken,
) -> Result<String, Attempt> {
    let mut stream = response.bytes_stream();
    let mut kept = Vec::new();
    while kept.len() < MAX_ERROR_BYTES {
        let next = tokio::select! {
            next = tokio::time::timeout(IDLE_TIMEOUT, stream.next()) => next,
            () = cancel.cancelled() => return Err(Attempt::Cancelled),
        };
        let Ok(Some(Ok(bytes))) = next else {
            break;
        };
        let room = MAX_ERROR_BYTES - kept.len();
        kept.extend_from_slice(&bytes[..bytes.len().min(room)]);
    }
    Ok(String::from_utf8_lossy(&kept).chars().take(500).collect())
}

/// Applies one SSE payload; returns true on `[DONE]`.
fn handle_payload(
    payload: &str,
    builder: &mut CompletionBuilder,
    on_delta: &mut (dyn FnMut(StreamDelta) + Send),
    shown: &mut bool,
) -> Result<bool, Attempt> {
    if payload.trim() == "[DONE]" {
        return Ok(true);
    }
    let Ok(chunk) = serde_json::from_str::<Value>(payload) else {
        return Ok(false);
    };
    if let Some(error) = chunk.get("error") {
        let message = error
            .get("message")
            .and_then(Value::as_str)
            .map_or_else(|| error.to_string(), str::to_string);
        return Err(Attempt::Retryable(format!("provider error: {message}")));
    }
    for delta in builder.apply(&chunk) {
        *shown = true;
        on_delta(delta);
    }
    Ok(false)
}

enum Attempt {
    Cancelled,
    /// 400/422: the request itself was refused (maybe an unsupported field).
    Rejected(String),
    Retryable(String),
    /// 429/503, with the server's `Retry-After` when it sent one.
    Throttled(String, Option<Duration>),
    Fatal(String),
}

/// `Retry-After` in seconds (the HTTP-date form is rare from model APIs and
/// falls back to the backoff).
fn parse_retry_after(value: &str) -> Option<Duration> {
    let seconds: f64 = value.trim().parse().ok()?;
    (seconds.is_finite() && seconds >= 0.0)
        .then(|| Duration::from_secs_f64(seconds.min(MAX_RETRY_WAIT.as_secs_f64())))
}

/// What the endpoint says about a model's size limits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ModelLimits {
    pub context_window: u64,
    pub max_output: Option<u64>,
}

/// Finds `model` in a `GET /models` listing and reads its limits.
pub fn model_limits_from_listing(listing: &serde_json::Value, model: &str) -> Option<ModelLimits> {
    let entry = listing
        .get("data")?
        .as_array()?
        .iter()
        .find(|entry| entry.get("id").and_then(serde_json::Value::as_str) == Some(model))?;
    let top = entry.get("top_provider");
    let number = |value: Option<&serde_json::Value>| value.and_then(serde_json::Value::as_u64);
    let context_window = number(entry.get("context_length"))
        .or_else(|| number(top.and_then(|top| top.get("context_length"))))
        .or_else(|| number(entry.get("max_model_len")))
        .or_else(|| number(entry.get("context_window")))
        .filter(|tokens| *tokens > 0)?;
    let max_output = number(top.and_then(|top| top.get("max_completion_tokens")))
        .or_else(|| number(entry.get("max_completion_tokens")))
        .or_else(|| number(entry.get("max_output_tokens")));
    Some(ModelLimits {
        context_window,
        max_output,
    })
}

#[cfg(test)]
#[path = "limits_tests.rs"]
mod limits_tests;

#[cfg(test)]
mod http_tests;
