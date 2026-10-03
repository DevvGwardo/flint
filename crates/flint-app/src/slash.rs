//! `/` commands typed into the composer.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SlashCommand {
    New,
    Clear,
    Model,
    Agent,
    Mode,
    Effort,
    Approval,
    Review,
    Help,
}

pub const COMMANDS: &[(SlashCommand, &str, &str)] = &[
    (SlashCommand::New, "/new", "Start a new session"),
    (
        SlashCommand::Clear,
        "/clear",
        "Clear this session's transcript",
    ),
    (
        SlashCommand::Model,
        "/model",
        "Change the model and endpoint",
    ),
    (
        SlashCommand::Agent,
        "/agent",
        "Switch agent: /agent claude, codex, droid or flint",
    ),
    (SlashCommand::Effort, "/effort", "Reasoning effort"),
    (
        SlashCommand::Mode,
        "/mode",
        "The agent's mode (or flint's auto-run)",
    ),
    (
        SlashCommand::Approval,
        "/approval",
        "Switch between auto-run and ask first",
    ),
    (SlashCommand::Review, "/review", "Open the changes panel"),
    (SlashCommand::Help, "/help", "Shortcuts and commands"),
];

/// The `/word` being typed when the composer holds only a command prefix.
pub fn active_query(text: &str) -> Option<&str> {
    let rest = text.strip_prefix('/')?;
    (!rest.contains(char::is_whitespace)).then_some(rest)
}

pub fn matches(query: &str) -> Vec<(SlashCommand, &'static str, &'static str)> {
    let query = query.to_lowercase();
    let mut found: Vec<_> = COMMANDS
        .iter()
        .copied()
        .filter(|(_, name, _)| name[1..].starts_with(&query))
        .collect();
    // An exact name wins over longer ones it prefixes (`/mode` vs `/model`).
    found.sort_by_key(|(_, name, _)| name[1..] != query);
    found
}
