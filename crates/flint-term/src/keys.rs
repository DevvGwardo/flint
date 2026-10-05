//! Key presses to the bytes a terminal program expects (xterm conventions):
//! arrows and Home/End (with application-cursor mode), function keys,
//! Ctrl combinations, Alt as an Esc prefix, and modifier parameters
//! (`ESC [ 1 ; 5 A` for Ctrl+Up).

/// Modifier keys held with a key.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Mods {
    pub shift: bool,
    pub ctrl: bool,
    pub alt: bool,
}

impl Mods {
    /// xterm's modifier parameter: 1 + shift + 2·alt + 4·ctrl.
    fn param(self) -> u8 {
        1 + u8::from(self.shift) + 2 * u8::from(self.alt) + 4 * u8::from(self.ctrl)
    }

    fn any(self) -> bool {
        self.shift || self.ctrl || self.alt
    }
}

/// Bytes for `key` (a key name like `"up"`, `"enter"`, `"f5"`, or a single
/// character). `app_cursor` is DECCKM (vim, htop, less set it). `None`
/// when the key produces nothing for the terminal (e.g. a bare modifier).
pub fn encode(key: &str, mods: Mods, app_cursor: bool) -> Option<Vec<u8>> {
    let csi_mod = |final_byte: char| -> Vec<u8> {
        if mods.any() {
            format!("\x1b[1;{}{final_byte}", mods.param()).into_bytes()
        } else if app_cursor {
            format!("\x1bO{final_byte}").into_bytes()
        } else {
            format!("\x1b[{final_byte}").into_bytes()
        }
    };
    let tilde = |code: u8| -> Vec<u8> {
        if mods.any() {
            format!("\x1b[{code};{}~", mods.param()).into_bytes()
        } else {
            format!("\x1b[{code}~").into_bytes()
        }
    };
    let alt_prefix = |bytes: Vec<u8>| -> Vec<u8> {
        if mods.alt {
            let mut out = vec![0x1b];
            out.extend(bytes);
            out
        } else {
            bytes
        }
    };
    let bytes = match key {
        "up" => csi_mod('A'),
        "down" => csi_mod('B'),
        "right" => csi_mod('C'),
        "left" => csi_mod('D'),
        "home" => csi_mod('H'),
        "end" => csi_mod('F'),
        "pageup" => tilde(5),
        "pagedown" => tilde(6),
        "insert" => tilde(2),
        "delete" => tilde(3),
        "enter" => alt_prefix(vec![b'\r']),
        "tab" if mods.shift => b"\x1b[Z".to_vec(),
        "tab" => alt_prefix(vec![b'\t']),
        "escape" => alt_prefix(vec![0x1b]),
        "backspace" if mods.ctrl => alt_prefix(vec![0x08]),
        "backspace" => alt_prefix(vec![0x7f]),
        "space" if mods.ctrl => vec![0],
        "space" => alt_prefix(vec![b' ']),
        "f1" | "f2" | "f3" | "f4" => {
            let final_byte = ['P', 'Q', 'R', 'S'][usize::from(key.as_bytes()[1] - b'1')];
            if mods.any() {
                format!("\x1b[1;{}{final_byte}", mods.param()).into_bytes()
            } else {
                format!("\x1bO{final_byte}").into_bytes()
            }
        }
        _ => {
            if let Some(code) = function_key_code(key) {
                tilde(code)
            } else {
                let mut chars = key.chars();
                let (Some(ch), None) = (chars.next(), chars.next()) else {
                    return None;
                };
                let base = if mods.ctrl {
                    ctrl_byte(ch).map(|b| vec![b])
                } else {
                    None
                };
                let plain = base.unwrap_or_else(|| ch.to_string().into_bytes());
                alt_prefix(plain)
            }
        }
    };
    Some(bytes)
}

/// `CSI n ~` codes for F5–F12.
fn function_key_code(key: &str) -> Option<u8> {
    Some(match key {
        "f5" => 15,
        "f6" => 17,
        "f7" => 18,
        "f8" => 19,
        "f9" => 20,
        "f10" => 21,
        "f11" => 23,
        "f12" => 24,
        _ => return None,
    })
}

/// The control byte for Ctrl+`ch` (Ctrl+A = 0x01 … Ctrl+_ = 0x1f).
fn ctrl_byte(ch: char) -> Option<u8> {
    match ch.to_ascii_lowercase() {
        c @ 'a'..='z' => Some(c as u8 - b'a' + 1),
        '@' | '2' => Some(0),
        '[' | '3' => Some(0x1b),
        '\\' | '4' => Some(0x1c),
        ']' | '5' => Some(0x1d),
        '^' | '6' => Some(0x1e),
        '_' | '-' | '7' => Some(0x1f),
        '8' | '?' => Some(0x7f),
        _ => None,
    }
}

/// Pasted text, wrapped for bracketed-paste mode and with line breaks as CR.
pub fn paste(text: &str, bracketed: bool) -> Vec<u8> {
    let normalized = text.replace("\r\n", "\r").replace('\n', "\r");
    if bracketed {
        // Removing one marker can synthesize another from nested fragments.
        // No escape byte may remain in the payload, including after removal.
        let safe = normalized.replace("\x1b[201~", "").replace('\x1b', "");
        format!("\x1b[200~{safe}\x1b[201~").into_bytes()
    } else {
        normalized.into_bytes()
    }
}

#[cfg(test)]
#[path = "keys_tests.rs"]
mod tests;
