//! Chat Completions client: request building, streaming, and retries.

pub mod stream;

use std::time::Duration;

use futures_util::StreamExt;
use serde_json::Value;
use serde_json::json;
use tokio_util::sync::CancellationToken;

pub use stream::Completion;
use stream::CompletionBuilder;
pub use stream::RawToolCall;
use stream::SseParser;
pub use stream::StreamDelta;

const MAX_RETRIES: u32 = 3;
const CONNECT_TIMEOUT: Duration = Duration::from_secs(20);
/// Longest silence allowed mid-stream before the request is abandoned.
const IDLE_TIMEOUT: Duration = Duration::from_secs(180);

/// One message of the conversation.
#[derive(Debug, Clone, PartialEq)]
pub enum Message {
    System(String),
    User(String),
    Assistant {
        content: String,
        /// Kept for DeepSeek thinking mode, which expects it back on
        /// tool-call turns within the same user turn.
        reasoning: String,
        tool_calls: Vec<RawToolCall>,
    },
    Tool {
        call_id: String,
        content: String,
    },
}

impl Message {
    /// Wire form. `with_reasoning` sends `reasoning_content` back.
    pub fn to_wire(&self, with_reasoning: bool) -> Value {
        match self {
            Message::System(text) => json!({"role": "system", "content": text}),
            Message::User(text) => json!({"role": "user", "content": text}),
            Message::Assistant {
                content,
                reasoning,
                tool_calls,
            } => {
                let mut message = json!({"role": "assistant", "content": content});
                if !tool_calls.is_empty() {
                    message["tool_calls"] = tool_calls
                        .iter()
                        .map(|call| {
                            json!({
                                "id": call.id,
                                "type": "function",
                                "function": {"name": call.name, "arguments": call.arguments},
                            })
                        })
                        .collect();
                }
                if with_reasoning && !reasoning.is_empty() {
                    message["reasoning_content"] = Value::String(reasoning.clone());
                }
                message
            }
            Message::Tool { call_id, content } => {
                json!({"role": "tool", "tool_call_id": call_id, "content": content})
            }
        }
    }
}

/// Why a model call failed.
#[derive(Debug, Clone, PartialEq)]
pub enum ProviderError {
    Cancelled,
    /// Retries exhausted or a non-retryable error.
    Failed(String),
}

/// An OpenAI-compatible Chat Completions endpoint.
#[derive(Debug, Clone)]
pub struct Provider {
    http: reqwest::Client,
    base_url: String,
    model: String,
    api_key: String,
}

impl Provider {
    pub fn new(base_url: &str, model: &str, api_key: &str) -> Self {
        let http = reqwest::Client::builder()
            .connect_timeout(CONNECT_TIMEOUT)
            .build()
            .unwrap_or_default();
        Self {
            http,
            base_url: base_url.trim_end_matches('/').to_string(),
            model: model.to_string(),
            api_key: api_key.to_string(),
        }
    }

    /// Streams one completion. `on_delta` sees text/reasoning as it arrives.
    /// Retries rate limits, server errors and dropped connections, but only
    /// before any delta was shown.
    pub async fn complete(
        &self,
        messages: &[Value],
        tools: &[Value],
        on_delta: &mut (dyn FnMut(StreamDelta) + Send),
        cancel: &CancellationToken,
    ) -> Result<Completion, ProviderError> {
        let mut body = json!({
            "model": self.model,
            "messages": messages,
            "stream": true,
            "stream_options": {"include_usage": true},
        });
        if !tools.is_empty() {
            body["tools"] = Value::Array(tools.to_vec());
            body["tool_choice"] = json!("auto");
        }
        let mut attempt = 0;
        loop {
            let mut shown = false;
            let result = self.attempt(&body, on_delta, &mut shown, cancel).await;
            match result {
                Ok(completion) => return Ok(completion),
                Err(Attempt::Cancelled) => return Err(ProviderError::Cancelled),
                Err(Attempt::Fatal(message)) => return Err(ProviderError::Failed(message)),
                Err(Attempt::Retryable(message)) => {
                    attempt += 1;
                    if shown || attempt > MAX_RETRIES {
                        return Err(ProviderError::Failed(message));
                    }
                    let backoff = Duration::from_millis(1000 * 2u64.pow(attempt - 1));
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
        body: &Value,
        on_delta: &mut (dyn FnMut(StreamDelta) + Send),
        shown: &mut bool,
        cancel: &CancellationToken,
    ) -> Result<Completion, Attempt> {
        let request = self
            .http
            .post(format!("{}/chat/completions", self.base_url))
            .bearer_auth(&self.api_key)
            .json(body)
            .send();
        let response = tokio::select! {
            response = request => response.map_err(|err| Attempt::Retryable(format!("request failed: {err}")))?,
            () = cancel.cancelled() => return Err(Attempt::Cancelled),
        };
        let status = response.status();
        if !status.is_success() {
            let text = response.text().await.unwrap_or_default();
            let snippet: String = text.chars().take(500).collect();
            let message = format!("provider returned {status}: {snippet}");
            let retryable = status.as_u16() == 429 || status.is_server_error();
            return Err(if retryable {
                Attempt::Retryable(message)
            } else {
                Attempt::Fatal(message)
            });
        }
        let mut bytes = response.bytes_stream();
        let mut parser = SseParser::default();
        let mut builder = CompletionBuilder::default();
        let mut done = false;
        while !done {
            let next = tokio::select! {
                next = tokio::time::timeout(IDLE_TIMEOUT, bytes.next()) => next,
                () = cancel.cancelled() => return Err(Attempt::Cancelled),
            };
            let chunk = match next {
                Err(_) => return Err(Attempt::Retryable("stream stalled".to_string())),
                Ok(None) => break,
                Ok(Some(Err(err))) => {
                    return Err(Attempt::Retryable(format!("stream broke: {err}")));
                }
                Ok(Some(Ok(chunk))) => chunk,
            };
            for payload in parser.push(&chunk) {
                done |= handle_payload(&payload, &mut builder, on_delta, shown)?;
            }
        }
        if let Some(payload) = parser.finish() {
            handle_payload(&payload, &mut builder, on_delta, shown)?;
        }
        Ok(builder.finish())
    }
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
    Retryable(String),
    Fatal(String),
}
