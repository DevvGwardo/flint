//! An ACP agent's own session options (model, reasoning, mode, fast, …):
//! which composer chip shows each one, the option menus, and setting a value
//! (`Op::SetSessionOption`). Rendering is in `option_chips.rs`.

use flint_agent::Op;
use flint_agent::SessionOption;
use gpui_kit::*;

use crate::app::FlintApp;

/// Where each option goes in the composer.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Slots {
    pub model: Option<SessionOption>,
    pub reasoning: Option<SessionOption>,
    pub mode: Option<SessionOption>,
    /// An on/off option shown as a toggle.
    pub fast: Option<SessionOption>,
    /// Everything else, under "More".
    pub more: Vec<SessionOption>,
}

pub fn slots(options: &[SessionOption]) -> Slots {
    let mut slots = Slots::default();
    for option in options {
        let place = match option.category.as_deref() {
            Some("model") if slots.model.is_none() => &mut slots.model,
            Some("thought_level") if slots.reasoning.is_none() => &mut slots.reasoning,
            Some("mode") if slots.mode.is_none() => &mut slots.mode,
            Some("model_config") if slots.fast.is_none() && is_toggle(option) => &mut slots.fast,
            _ => {
                slots.more.push(option.clone());
                continue;
            }
        };
        *place = Some(option.clone());
    }
    slots
}

/// Two choices that read as on/off.
pub fn is_toggle(option: &SessionOption) -> bool {
    option.choices.len() == 2
        && option
            .choices
            .iter()
            .all(|c| matches!(c.value.as_str(), "on" | "off" | "true" | "false"))
}

pub fn toggle_is_on(option: &SessionOption) -> bool {
    matches!(option.current.as_str(), "on" | "true")
}

/// The value that flips a toggle.
pub fn toggled_value(option: &SessionOption) -> Option<String> {
    option
        .choices
        .iter()
        .find(|c| c.value != option.current)
        .map(|c| c.value.clone())
}

/// Modes that let the agent act without asking; Shift+Tab never lands on
/// them (they stay selectable from the menu).
pub fn is_permissive(value: &str) -> bool {
    let value = value.to_lowercase();
    [
        "bypass",
        "full-access",
        "full_access",
        "dontask",
        "yolo",
        "auto-high",
        "auto_high",
        "skip-permissions",
    ]
    .iter()
    .any(|word| value.contains(word))
}

/// The next mode for Shift+Tab, skipping permissive modes.
pub fn next_mode(option: &SessionOption) -> Option<String> {
    let safe: Vec<&str> = option
        .choices
        .iter()
        .map(|c| c.value.as_str())
        .filter(|v| !is_permissive(v))
        .collect();
    if safe.is_empty() {
        return None;
    }
    let next = match safe.iter().position(|v| *v == option.current) {
        Some(i) => safe[(i + 1) % safe.len()],
        None => safe[0],
    };
    (next != option.current).then(|| next.to_string())
}

/// What an option menu lists.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MenuTarget {
    /// The choices of one option.
    Option(String),
    /// The options that have no chip of their own.
    More,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OptionMenu {
    pub target: MenuTarget,
    pub selected: usize,
}

impl FlintApp {
    pub fn session_slots(&self) -> Slots {
        slots(&self.session().options)
    }

    /// Opens the menu for an option (or "More"), selecting its current value.
    pub fn open_option_menu(&mut self, target: MenuTarget, cx: &mut Context<Self>) {
        let selected = match &target {
            MenuTarget::Option(id) => self
                .session()
                .options
                .iter()
                .find(|o| &o.id == id)
                .and_then(|o| o.choices.iter().position(|c| c.value == o.current))
                .unwrap_or(0),
            MenuTarget::More => 0,
        };
        self.option_menu = Some(OptionMenu { target, selected });
        self.option_menu_scroll.set_offset(Point::default());
        self.option_menu_scroll.scroll_to_item(selected);
        self.option_menu_needs_scroll.set(true);
        self.agent_menu = false;
        self.slash = None;
        self.mention = None;
        cx.notify();
    }

    /// Rows of the open menu: (value or option id, label, detail, current).
    pub fn option_menu_rows(&self) -> Vec<(String, String, Option<String>, bool)> {
        let Some(menu) = &self.option_menu else {
            return Vec::new();
        };
        match &menu.target {
            MenuTarget::Option(id) => self
                .session()
                .options
                .iter()
                .find(|o| &o.id == id)
                .map(|o| {
                    o.choices
                        .iter()
                        .map(|c| {
                            (
                                c.value.clone(),
                                c.name.clone(),
                                c.description.clone(),
                                c.value == o.current,
                            )
                        })
                        .collect()
                })
                .unwrap_or_default(),
            MenuTarget::More => self
                .session_slots()
                .more
                .iter()
                .map(|o| {
                    let current = o
                        .current_choice()
                        .map_or_else(|| o.current.clone(), |c| c.name.clone());
                    (o.id.clone(), o.name.clone(), Some(current), false)
                })
                .collect(),
        }
    }

    /// Picks row `ix` of the open menu.
    pub fn pick_option_row(&mut self, ix: usize, cx: &mut Context<Self>) {
        let Some(menu) = self.option_menu.clone() else {
            return;
        };
        let Some((key, ..)) = self.option_menu_rows().into_iter().nth(ix) else {
            return;
        };
        match menu.target {
            MenuTarget::Option(id) => {
                self.option_menu = None;
                self.set_session_option(&id, &key, cx);
            }
            MenuTarget::More => self.open_option_menu(MenuTarget::Option(key), cx),
        }
    }

    /// Sends `Op::SetSessionOption`, starting the agent if it isn't running.
    /// The chip shows the new value right away; the agent's reply confirms it.
    pub fn set_session_option(&mut self, id: &str, value: &str, cx: &mut Context<Self>) {
        let ix = self.active;
        if let Some(option) = self.sessions[ix].options.iter_mut().find(|o| o.id == id) {
            option.current = value.to_string();
        }
        if self.sessions[ix].ops.is_none()
            && let Err(err) = self.ensure_engine(ix, cx)
        {
            self.apply_event(ix, flint_agent::AgentEvent::Error(format!("{err:#}")), cx);
        }
        if let Some(ops) = &self.sessions[ix].ops {
            ops.try_send(Op::SetSessionOption {
                id: id.to_string(),
                value: value.to_string(),
            })
            .ok();
        }
        cx.notify();
    }

    /// Shift+Tab for a session whose agent has modes. Returns false when
    /// there is no agent mode (flint's own auto-run toggle applies).
    pub fn cycle_agent_mode(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(mode) = self.session_slots().mode else {
            return false;
        };
        if let Some(next) = next_mode(&mode) {
            self.set_session_option(&mode.id, &next, cx);
        }
        true
    }

    /// Keys while an option menu is open. Returns whether it was consumed.
    pub fn option_menu_key(&mut self, key: &str, cx: &mut Context<Self>) -> bool {
        if self.option_menu.is_none() {
            return false;
        }
        let rows = self.option_menu_rows().len();
        let Some(menu) = self.option_menu.as_mut() else {
            return false;
        };
        match key {
            "up" => menu.selected = menu.selected.saturating_sub(1),
            "down" => menu.selected = (menu.selected + 1).min(rows.saturating_sub(1)),
            "enter" | "tab" => {
                let ix = menu.selected;
                self.pick_option_row(ix, cx);
            }
            "escape" => self.option_menu = None,
            _ => return false,
        }
        if let Some(menu) = &self.option_menu {
            self.option_menu_scroll.scroll_to_item(menu.selected);
        }
        cx.notify();
        true
    }
}

#[cfg(test)]
#[path = "session_options_tests.rs"]
mod tests;
