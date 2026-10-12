//! Command on macOS, Control elsewhere. Never bind Windows' logo key as Ctrl.

use gpui_kit::Modifiers;
use std::borrow::Cow;

pub fn primary(modifiers: Modifiers) -> bool {
    if cfg!(target_os = "macos") {
        modifiers.platform
    } else {
        modifiers.control
    }
}

pub fn label(mac_label: &'static str) -> Cow<'static, str> {
    if cfg!(target_os = "macos") {
        Cow::Borrowed(mac_label)
    } else {
        Cow::Owned(
            mac_label
                .replace('⌘', "Ctrl+")
                .replace('⇧', "Shift+")
                .replace('⌃', "Ctrl+")
                .replace('⏎', "Enter"),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui_kit::Keystroke;

    #[test]
    fn native_primary_matches_secondary_keybindings() {
        let primary_key = Keystroke::parse("secondary-enter").unwrap();
        assert!(primary(primary_key.modifiers));
        assert!(!primary(Modifiers::default()));
        if cfg!(target_os = "macos") {
            assert_eq!(label("⌘⇧R"), "⌘⇧R");
        } else {
            assert_eq!(label("⌘⇧R"), "Ctrl+Shift+R");
            assert!(!primary(Keystroke::parse("win-enter").unwrap().modifiers));
        }
    }
}
