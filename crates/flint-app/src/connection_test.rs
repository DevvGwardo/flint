//! Explicit, read-only provider checks. Never log a URL, key, response, or request.
use reqwest::{Client, Url, redirect::Policy};
use std::time::Duration;
use tokio_util::sync::CancellationToken;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConnectionResult {
    pub reachable: bool,
    /// /models may be public. A successful listing does not prove authentication.
    pub credentials_accepted: Option<bool>,
    pub model_listed: Option<bool>,
    pub message: &'static str,
}

impl ConnectionResult {
    fn failure(reachable: bool, message: &'static str) -> Self {
        Self {
            reachable,
            credentials_accepted: None,
            model_listed: None,
            message,
        }
    }

    pub fn label(&self) -> String {
        format!(
            "Endpoint: {} · Credentials: {} · Model: {}. {} Inference was not tested.",
            if self.reachable {
                "reachable"
            } else {
                "not verified"
            },
            match self.credentials_accepted {
                Some(true) => "accepted",
                Some(false) => "rejected",
                None => "not verified",
            },
            match self.model_listed {
                Some(true) => "listed",
                Some(false) => "not listed",
                None => "not verified",
            },
            self.message
        )
    }
}

/// Reject URL-embedded secrets and disable query/fragment forwarding.
pub fn models_url(base: &str) -> Result<Url, &'static str> {
    let mut url = Url::parse(base).map_err(|_| "Enter an http(s) endpoint with a host.")?;
    if !matches!(url.scheme(), "http" | "https")
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err("Use an http(s) endpoint without embedded credentials, query, or fragment.");
    }
    url.set_path(&format!("{}/models", url.path().trim_end_matches('/')));
    Ok(url)
}

pub struct Probe {
    cancel: CancellationToken,
    pub result: async_channel::Receiver<ConnectionResult>,
}

impl Drop for Probe {
    fn drop(&mut self) {
        self.cancel.cancel();
    }
}

impl Probe {
    pub fn start(base: &str, model: String, key: Option<String>) -> Result<Self, &'static str> {
        let url = models_url(base)?;
        let cancel = CancellationToken::new();
        let stopping = cancel.clone();
        let (tx, result) = async_channel::bounded(1);
        std::thread::Builder::new()
            .name("flint-connection-test".into())
            .spawn(move || {
                let Ok(runtime) = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                else {
                    let _ = tx.try_send(ConnectionResult::failure(
                        false,
                        "Couldn't start connection test.",
                    ));
                    return;
                };
                runtime.block_on(async move {
                    tokio::select! {
                        _ = stopping.cancelled() => {},
                        result = check(url, &model, key.as_deref(), Duration::from_secs(6)) => {
                            let _ = tx.try_send(result);
                        }
                    }
                });
            })
            .map_err(|_| "Couldn't start connection test.")?;
        Ok(Self { cancel, result })
    }
}

async fn check(url: Url, model: &str, key: Option<&str>, timeout: Duration) -> ConnectionResult {
    let Ok(client) = Client::builder()
        .redirect(Policy::none())
        .timeout(timeout)
        .no_proxy()
        .build()
    else {
        return ConnectionResult::failure(false, "Couldn't create connection test.");
    };
    let mut request = client.get(url);
    if let Some(key) = key {
        request = request.bearer_auth(key);
    }
    let mut response = match request.send().await {
        Ok(response) => response,
        Err(error) => {
            return ConnectionResult::failure(
                false,
                if error.is_timeout() {
                    "Connection test timed out."
                } else {
                    "Connection failed."
                },
            );
        }
    };
    let status = response.status();
    if status == 401 || status == 403 {
        return ConnectionResult {
            credentials_accepted: key.map(|_| false),
            ..ConnectionResult::failure(
                true,
                if key.is_some() {
                    "Authentication was refused."
                } else {
                    "Authentication is required; no credential was tested."
                },
            )
        };
    }
    if status.is_redirection() {
        return ConnectionResult::failure(
            true,
            "Redirect blocked; credentials were not forwarded.",
        );
    }
    if status == 404 || status == 405 || status == 501 {
        return ConnectionResult::failure(true, "This endpoint does not support GET /models.");
    }
    if !status.is_success() {
        return ConnectionResult::failure(true, "Model-list request was not successful.");
    }
    let mut bytes = Vec::new();
    loop {
        match response.chunk().await {
            Ok(Some(chunk)) if bytes.len() + chunk.len() <= 1024 * 1024 => {
                bytes.extend_from_slice(&chunk)
            }
            Ok(Some(_)) => {
                return ConnectionResult::failure(true, "Model list exceeded the 1 MiB limit.");
            }
            Ok(None) => break,
            Err(error) => {
                return ConnectionResult::failure(
                    true,
                    if error.is_timeout() {
                        "Connection test timed out."
                    } else {
                        "Couldn't read model list."
                    },
                );
            }
        }
    }
    let Ok(json) = serde_json::from_slice::<serde_json::Value>(&bytes) else {
        return ConnectionResult::failure(true, "Model-list response was malformed.");
    };
    let Some(list) = json.get("data").and_then(|value| value.as_array()) else {
        return ConnectionResult::failure(true, "Model-list response has an unsupported format.");
    };
    if list
        .iter()
        .any(|entry| entry.get("id").and_then(|id| id.as_str()).is_none())
    {
        return ConnectionResult::failure(true, "Model-list response was malformed.");
    }
    ConnectionResult {
        reachable: true,
        // An unauthenticated /models can return exactly the same response.
        credentials_accepted: None,
        model_listed: Some(list.iter().any(|entry| entry["id"].as_str() == Some(model))),
        message: if key.is_some() {
            "GET /models succeeded with a credential; this endpoint may be public, so authentication is not proven."
        } else {
            "GET /models succeeded without credentials; no credential was tested."
        },
    }
}

#[cfg(test)]
#[path = "connection_test_tests.rs"]
mod tests;
