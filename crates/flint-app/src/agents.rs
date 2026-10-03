//! Choosing who runs a session: flint's own engine or an ACP agent (Claude
//! Code, Codex). The choice applies to a session before its first message;
//! after that the session keeps its agent and a new one is opened instead.

use flint_acp::AcpAgent;
use flint_acp::AcpConfig;
use flint_agent::AgentKind;
use gpui_kit::*;

use crate::app::FlintApp;

/// Agents offered by the picker, in order.
pub const AGENTS: [AgentKind; 3] = [AgentKind::Flint, AgentKind::ClaudeCode, AgentKind::Codex];

/// `/agent claude|codex|flint` typed into the composer.
pub fn parse_command(text: &str) -> Option<AgentKind> {
    let name = text.trim().strip_prefix("/agent")?.trim().to_lowercase();
    match name.as_str() {
        "claude" | "claude-code" | "claude code" | "cc" => Some(AgentKind::ClaudeCode),
        "codex" => Some(AgentKind::Codex),
        "flint" => Some(AgentKind::Flint),
        _ => None,
    }
}

/// One line describing what an agent is, for the picker.
pub fn about(kind: AgentKind) -> &'static str {
    match kind {
        AgentKind::Flint => "flint's own agent and harness",
        AgentKind::ClaudeCode => "Anthropic's agent, on your Claude plan (ACP)",
        AgentKind::Codex => "OpenAI's agent, on your ChatGPT plan (ACP)",
    }
}

impl FlintApp {
    /// The composer chip text: the agent, plus the model for flint.
    pub fn agent_label(&self, kind: AgentKind) -> String {
        match kind {
            AgentKind::Flint => format!("flint · {}", self.model),
            AgentKind::ClaudeCode | AgentKind::Codex => kind.label().to_string(),
        }
    }

    pub fn toggle_agent_menu(&mut self, cx: &mut Context<Self>) {
        self.agent_menu = !self.agent_menu;
        self.slash = None;
        self.mention = None;
        cx.notify();
    }

    /// Picks the agent for the active session if it hasn't started yet,
    /// otherwise opens a new session with it.
    pub fn choose_agent(&mut self, kind: AgentKind, window: &mut Window, cx: &mut Context<Self>) {
        self.agent_menu = false;
        let fresh = {
            let session = self.session();
            session.view.items.is_empty() && session.ops.is_none()
        };
        if fresh {
            let ix = self.active;
            self.sessions[ix].agent = kind;
            self.start_agent_early(ix, cx);
            self.composer
                .update(cx, |state, cx| state.focus(window, cx));
            cx.notify();
        } else if self.session().agent != kind {
            self.new_agent_session(kind, window, cx);
        } else {
            cx.notify();
        }
    }

    /// A new session run by `kind` (⌘K and `/agent`).
    pub fn new_agent_session(
        &mut self,
        kind: AgentKind,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.agent_menu = false;
        self.new_session(window, cx);
        let ix = self.active;
        self.sessions[ix].agent = kind;
        self.start_agent_early(ix, cx);
        cx.notify();
    }

    /// Starts an ACP agent right away (its adapter takes 20–50 s), so its
    /// options are ready by the first message. Opening a session uses no
    /// plan quota; only prompts do.
    fn start_agent_early(&mut self, ix: usize, cx: &mut Context<Self>) {
        let session = &self.sessions[ix];
        if !self.options.start_agents_early
            || session.agent == AgentKind::Flint
            || session.ops.is_some()
        {
            return;
        }
        if !self.options.ephemeral() && session.dir.is_none() {
            self.sessions[ix].dir =
                Some(crate::store::sessions_dir(&self.home).join(crate::store::new_id()));
        }
        if let Err(err) = self.ensure_engine(ix, cx) {
            self.apply_event(ix, flint_agent::AgentEvent::Error(format!("{err:#}")), cx);
        }
    }

    /// Starts an ACP agent for a session (see `ensure_engine`).
    pub(crate) fn spawn_acp(
        &self,
        ix: usize,
        kind: AgentKind,
    ) -> Option<flint_agent::SessionHandle> {
        let agent = AcpAgent::for_kind(kind)?;
        let session = &self.sessions[ix];
        Some(flint_acp::spawn_acp_session(
            agent,
            AcpConfig {
                workspace: session.workspace.clone(),
                session_dir: session.dir.clone(),
                approval: self.approval,
            },
        ))
    }
}

#[cfg(test)]
mod tests {
    use flint_agent::AgentKind;
    use pretty_assertions::assert_eq;

    use super::parse_command;

    #[test]
    fn agent_commands() {
        assert_eq!(parse_command("/agent claude"), Some(AgentKind::ClaudeCode));
        assert_eq!(parse_command(" /agent Codex "), Some(AgentKind::Codex));
        assert_eq!(parse_command("/agent flint"), Some(AgentKind::Flint));
        assert_eq!(parse_command("/agent gpt"), None);
        assert_eq!(parse_command("/agents claude"), None);
        assert_eq!(parse_command("tell the agent claude"), None);
    }
}
