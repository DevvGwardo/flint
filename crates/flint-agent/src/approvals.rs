//! One approval inbox for the parent and all children, addressed by call id.
//!
//! "Approve always" releases every call waiting at that moment (the front end
//! marks them all approved) but only pre-approves later calls that match it:
//! any edit after an edit, or an explicitly allowlisted cargo build/check
//! subcommand (`cargo test …` after `cargo test --lib`). All other programs,
//! executable path aliases and nontrivial shell syntax need exact matches.
//! Configuration-overriding cargo flags need exact matches too. A fetch approves later
//! fetches from the same host; an MCP tool approves later calls of itself.

use std::collections::HashMap;
use std::sync::Mutex;

use tokio::sync::oneshot;

use crate::protocol::ApprovalDecision;
use crate::protocol::ToolKind;

/// What a call asks to do, for matching "approve always" rules.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Request {
    Edit,
    Command(String),
    /// `fetch_url` to this host.
    Fetch(String),
    /// An MCP tool, by its full name.
    Mcp(String),
}

impl Request {
    pub fn new(kind: ToolKind, command: Option<&str>) -> Self {
        match (kind, command) {
            (ToolKind::Command, Some(command)) => Self::Command(command.trim().to_string()),
            _ => Self::Edit,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Rule {
    AllEdits,
    /// The command's leading words.
    Prefix(Vec<String>),
    Exact(String),
    Host(String),
    Tool(String),
}

impl Rule {
    fn from(request: &Request) -> Self {
        match request {
            Request::Edit => Self::AllEdits,
            Request::Fetch(host) => Self::Host(host.clone()),
            Request::Mcp(name) => Self::Tool(name.clone()),
            Request::Command(command) => match command_prefix(command) {
                Some(prefix) => Self::Prefix(prefix),
                None => Self::Exact(command.clone()),
            },
        }
    }

    fn allows(&self, request: &Request) -> bool {
        match (self, request) {
            (Self::AllEdits, Request::Edit) => true,
            (Self::Host(rule), Request::Fetch(host)) => !host.is_empty() && rule == host,
            (Self::Tool(rule), Request::Mcp(name)) => rule == name,
            (Self::Exact(rule), Request::Command(command)) => rule == command,
            (Self::Prefix(prefix), Request::Command(command)) => {
                command_prefix(command).as_ref() == Some(prefix)
            }
            _ => false,
        }
    }
}

fn has_shell_syntax(command: &str) -> bool {
    command.chars().any(|c| {
        !(c.is_ascii_alphanumeric() || matches!(c, ' ' | '-' | '_' | '.' | '/' | ':' | '='))
    })
}

/// `["cargo", "test"]` for `cargo test --lib`; otherwise exact approval unless
/// both the executable and subcommand are explicitly allowlisted.
fn command_prefix(command: &str) -> Option<Vec<String>> {
    if has_shell_syntax(command) {
        return None;
    }
    let mut words = command.split_whitespace();
    let program = words.next()?;
    // Do not normalize executable spellings: /bin/rm, ./rm, quoted words,
    // interpreters and unknown executables all need exact approval. Keep
    // the established cargo build/check workflow as an explicit exception.
    if program != "cargo" {
        return None;
    }
    let sub = words.next()?;
    if !matches!(sub, "test" | "build" | "check" | "clippy" | "fmt" | "bench") {
        return None;
    }
    // Configuration overrides can substitute programs and change execution
    // behavior. Never generalize such a command into a prefix rule.
    if words.any(|word| word.starts_with("--config") || word.starts_with("-Z")) {
        return None;
    }
    Some(vec![program.to_string(), sub.to_string()])
}

#[derive(Default)]
pub(crate) struct Approvals {
    inner: Mutex<Inner>,
}

#[derive(Default)]
struct Inner {
    rules: Vec<Rule>,
    pending: HashMap<String, (Request, oneshot::Sender<ApprovalDecision>)>,
}

impl Approvals {
    fn inner(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Waits for an answer, or `None` when a rule already approves it.
    pub fn register(
        &self,
        id: &str,
        request: Request,
    ) -> Option<oneshot::Receiver<ApprovalDecision>> {
        let mut inner = self.inner();
        if inner.rules.iter().any(|rule| rule.allows(&request)) {
            return None;
        }
        let (tx, rx) = oneshot::channel();
        inner.pending.insert(id.to_string(), (request, tx));
        Some(rx)
    }

    pub fn remove(&self, id: &str) {
        self.inner().pending.remove(id);
    }

    pub fn respond(&self, id: &str, decision: ApprovalDecision) {
        let mut inner = self.inner();
        if let Some((request, tx)) = inner.pending.remove(id) {
            if decision == ApprovalDecision::ApproveAlways {
                let rule = Rule::from(&request);
                if !inner.rules.contains(&rule) {
                    inner.rules.push(rule);
                }
                // The front end shows every waiting card as approved.
                for (_, (_, waiting)) in inner.pending.drain() {
                    let _ = waiting.send(ApprovalDecision::Approve);
                }
            }
            let _ = tx.send(decision);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn command(text: &str) -> Request {
        Request::Command(text.to_string())
    }

    #[tokio::test]
    async fn approve_always_releases_pending_calls_and_scopes_future_ones() {
        let approvals = Approvals::default();
        let first = approvals
            .register("agent-1:a", command("cargo test --lib"))
            .expect("first");
        let second = approvals
            .register("agent-2:b", Request::Edit)
            .expect("second");
        approvals.respond("unknown", ApprovalDecision::ApproveAlways);
        assert!(approvals.register("probe", command("cargo test")).is_some());
        approvals.remove("probe");
        approvals.respond("agent-1:a", ApprovalDecision::ApproveAlways);
        assert_eq!(first.await.expect("first"), ApprovalDecision::ApproveAlways);
        assert_eq!(second.await.expect("second"), ApprovalDecision::Approve);
        assert!(
            approvals
                .register("c", command("cargo test -p x"))
                .is_none()
        );
        assert!(approvals.register("d", command("cargo build")).is_some());
        assert!(
            approvals
                .register("e", command("cargo test && rm -rf ~"))
                .is_some()
        );
        assert!(approvals.register("f", Request::Edit).is_some());
    }

    #[test]
    fn executable_spellings_and_unknown_programs_are_exact_only() {
        for command_text in [
            "/bin/rm file",
            "./rm file",
            "'rm' file",
            "\"rm\" file",
            "/usr/bin/python3 script.py",
            "unknown-program arg",
        ] {
            let rule = Rule::from(&command(command_text));
            assert_eq!(rule, Rule::Exact(command_text.into()));
            assert!(!rule.allows(&command(&format!("{command_text} another"))));
        }
    }

    #[test]
    fn rules_match_prefixes_edits_and_exact_commands() {
        let rule = Rule::from(&command("npm run build"));
        assert_eq!(rule, Rule::Exact("npm run build".into()));
        assert!(!rule.allows(&command("npm run lint")));
        assert!(!rule.allows(&command("npm install evil")));
        assert!(!rule.allows(&Request::Edit));

        let rule = Rule::from(&command("make check"));
        assert_eq!(rule, Rule::Exact("make check".into()));
        let rule = Rule::from(&command("python script.py"));
        assert!(!rule.allows(&command("python other.py")));

        let rule = Rule::from(&command("rm build/out.txt"));
        assert!(rule.allows(&command("rm build/out.txt")));
        assert!(!rule.allows(&command("rm -rf /")));

        let rule = Rule::from(&command("cargo test | tail -5"));
        assert!(!rule.allows(&command("cargo test")));
        assert!(rule.allows(&command("cargo test | tail -5")));

        assert!(Rule::from(&Request::Edit).allows(&Request::Edit));

        let rule = Rule::from(&Request::Fetch("docs.rs".into()));
        assert!(rule.allows(&Request::Fetch("docs.rs".into())));
        assert!(!rule.allows(&Request::Fetch("evil.example".into())));
        let rule = Rule::from(&Request::Mcp("mcp__gh__list".into()));
        assert!(rule.allows(&Request::Mcp("mcp__gh__list".into())));
        assert!(!rule.allows(&Request::Mcp("mcp__gh__delete".into())));
    }
}
