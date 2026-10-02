//! Optional Typesafe System One ("JEV") judgment client.
//!
//! JEV answers calibrated yes/no ("noul") questions about a small JSON state.
//! The harness spends one to turn a cheap heuristic into a verdict. In the
//! author's benchmark ablation, disabling it cost 14 points (89% -> 75%).
//!
//! Wire shape:
//!   POST {base}/v1/systemone   Authorization: Bearer <key>
//!   {"model":"jev-latest","state":{…},"questions":{"stuck":{"type":"noul","instructions":"…","criteria":{"true":"…","false":"…"}}}}
//!   -> {"answers":{"stuck":{"type":"noul","noul":0.79}}}
//!
//! Contract: every failure resolves to `None`; judgment never fails a turn.

use std::time::Duration;

use serde_json::Value;
use serde_json::json;

use crate::protocol::JevConfig;

const TIMEOUT: Duration = Duration::from_secs(5);

/// A calibrated yes/no question with both outcomes spelled out.
#[derive(Debug, Clone, Copy)]
pub struct NoulQuestion {
    pub instructions: &'static str,
    pub yes: &'static str,
    pub no: &'static str,
}

pub const STUCK_QUESTION: NoulQuestion = NoulQuestion {
    instructions: "Is this agent turn stuck in a loop — repeating the same action or the same \
                   failing command without making progress?",
    yes: "stuck — the same call or the same failure keeps repeating with no new information",
    no: "not stuck — calls differ, or each one reacts to what the previous one returned",
};

/// Covers deliverables beyond the edited files.
pub const VERIFY_QUESTION: NoulQuestion = NoulQuestion {
    instructions: "Is the work in this turn actually verified — were the edited files, created \
                   manifests, and required deliverables checked by a command or validation script \
                   that ran after the last change?",
    yes: "verified — a build, test, lint or validation ran after the final edit and its result is \
          consistent",
    no: "unverified — files changed with no check run afterwards, the checks ran before the \
         edits, or a required deliverable was never checked",
};

pub const WATCHDOG_QUESTION: NoulQuestion = NoulQuestion {
    instructions: "Does this task require modifying files, and has the agent not yet done so?",
    yes: "requires changes — the task asks to create or modify files, but no files have been \
          modified yet",
    no: "does not require changes — the user only asked a question or no file modifications are \
         needed",
};

/// HTTP client for the System One endpoint.
#[derive(Debug, Clone)]
pub struct JevClient {
    config: JevConfig,
    http: reqwest::Client,
}

impl JevClient {
    pub fn new(config: JevConfig) -> Self {
        Self {
            config,
            http: reqwest::Client::new(),
        }
    }

    /// Probability in `[0, 1]` that the answer is yes, or `None` without a verdict.
    pub async fn ask_noul(&self, state: &Value, name: &str, question: NoulQuestion) -> Option<f64> {
        let body = json!({
            "model": self.config.model,
            "state": state,
            "questions": {
                name: {
                    "type": "noul",
                    "instructions": question.instructions,
                    "criteria": {"true": question.yes, "false": question.no},
                }
            }
        });
        let url = format!(
            "{}/v1/systemone",
            self.config.base_url.trim_end_matches('/')
        );
        let response = self
            .http
            .post(url)
            .bearer_auth(&self.config.api_key)
            .timeout(TIMEOUT)
            .json(&body)
            .send()
            .await
            .ok()?;
        if !response.status().is_success() {
            return None;
        }
        let payload: Value = response.json().await.ok()?;
        let answer = payload.get("answers")?.get(name)?;
        let value = answer
            .get("noul")
            .or_else(|| answer.get("p_yes"))?
            .as_f64()?;
        value.is_finite().then(|| value.clamp(0.0, 1.0))
    }
}
